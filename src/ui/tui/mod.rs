use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::Ui;

#[derive(Clone)]
pub struct Tui {
    state: Arc<(Mutex<State>, Condvar)>,
}

#[derive(Default)]
struct State {
    pending_lines: Vec<Line<'static>>,
    input_responses: VecDeque<String>,
    input_buffer: String,
    notifications: Vec<(String, String)>,
    shutdown: bool,
}

impl Tui {
    pub fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
        }
    }

    pub fn stop(&self) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap_or_else(|e| e.into_inner()).shutdown = true;
        wake.notify_all();
    }

    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(3),
            },
        )?;
        let result = self.run_terminal(&mut terminal);
        disable_raw_mode()?;
        let last_row = terminal.size()?.height - 1;
        execute!(terminal.backend_mut(), MoveTo(0, last_row), Print("\r\n"))?;
        terminal.show_cursor()?;
        result
    }

    fn run_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        terminal.insert_before(1, |buffer| {
            Paragraph::new("").render(buffer.area, buffer);
        })?;
        let mut viewport_height = 3;
        let mut history_has_content = false;
        loop {
            let (pending_lines, shutdown, notification_count) = {
                let mut state = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
                (
                    std::mem::take(&mut state.pending_lines),
                    state.shutdown,
                    state.notifications.len(),
                )
            };
            history_has_content |= !pending_lines.is_empty();
            let next_height = notification_count as u16
                + 3
                + u16::from(history_has_content && notification_count > 0);
            if next_height > viewport_height {
                let added_rows = next_height - viewport_height;
                let screen_height = terminal.size()?.height;
                let last_row = screen_height - 1;
                let viewport_top = screen_height - next_height;
                execute!(
                    terminal.backend_mut(),
                    MoveTo(0, last_row),
                    Print("\r\n".repeat(added_rows as usize)),
                    MoveTo(0, viewport_top),
                )?;
                *terminal = Terminal::with_options(
                    CrosstermBackend::new(io::stdout()),
                    TerminalOptions {
                        viewport: Viewport::Inline(next_height),
                    },
                )?;
                terminal.clear()?;
                viewport_height = next_height;
            }
            if !pending_lines.is_empty() {
                let width = terminal.size()?.width as usize;
                let height = pending_lines
                    .iter()
                    .map(|line| line.width().max(1).div_ceil(width))
                    .sum::<usize>() as u16;
                let paragraph = Paragraph::new(pending_lines).wrap(Wrap { trim: false });
                terminal.insert_before(height, move |buffer| {
                    paragraph.render(buffer.area, buffer);
                })?;
            }
            terminal.draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ])
                    .split(area);
                let (lock, _) = &*self.state;
                let state = lock.lock().unwrap_or_else(|e| e.into_inner());
                let mut lines = Vec::new();
                for (title, notice) in &state.notifications {
                    lines.push(Line::from(Span::styled(
                        format!("{title}: {notice}"),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                if history_has_content && !lines.is_empty() {
                    lines.insert(0, Line::default());
                }
                let content = Paragraph::new(lines).wrap(Wrap { trim: false });
                frame.render_widget(content, chunks[0]);
                let input = Paragraph::new(state.input_buffer.as_str())
                    .style(Style::default().bg(Color::DarkGray).fg(Color::White));
                frame.render_widget(input, chunks[2]);
                frame.set_cursor_position((
                    chunks[2]
                        .x
                        .saturating_add(state.input_buffer.chars().count() as u16),
                    chunks[2].y,
                ));
            })?;
            if shutdown {
                return Ok(());
            }

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)
                    {
                        self.stop();
                        return Ok(());
                    }
                    let (lock, wake) = &*self.state;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    match key.code {
                        KeyCode::Enter => {
                            let input = std::mem::take(&mut state.input_buffer);
                            state.input_responses.push_back(input);
                            wake.notify_all();
                        }
                        KeyCode::Char(ch) => state.input_buffer.push(ch),
                        KeyCode::Backspace => {
                            state.input_buffer.pop();
                        }
                        KeyCode::Esc => {
                            state.input_responses.push_back(String::new());
                            wake.notify_all();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn append_message_lines(lines: &mut Vec<Line<'static>>, message: &crate::core::Message) {
    use crate::core::{
        AssistantContent, Message, SystemContent, ToolResultContent, UserContent, UserContentBlock,
    };
    match message {
        Message::System(message) => match &message.content {
            SystemContent::Text(text) => lines.push(Line::from(format!("system> {text}"))),
            SystemContent::Blocks(blocks) => {
                for block in blocks {
                    lines.push(Line::from(format!("system> {}", block.text)));
                }
            }
        },
        Message::User(message) => match &message.content {
            UserContent::Text(text) => lines.push(Line::from(format!("you> {text}"))),
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        UserContentBlock::Text(text) => {
                            lines.push(Line::from(format!("you> {}", text.text)))
                        }
                        UserContentBlock::Image(image) => {
                            lines.push(Line::from(format!("you> [image: {}]", image.mime_type)))
                        }
                    }
                }
            }
        },
        Message::Assistant(message) => {
            for content in &message.content {
                match content {
                    AssistantContent::Text(text) => {
                        lines.push(Line::from(format!("assistant> {}", text.text)))
                    }
                    AssistantContent::Thinking(thinking) => {
                        lines.push(Line::from(format!("thinking> {}", thinking.thinking)))
                    }
                    AssistantContent::ToolCall(call) => lines.push(Line::from(format!(
                        "tool call> {} {:?}",
                        call.name, call.arguments
                    ))),
                }
            }
        }
        Message::ToolResult(message) => {
            for content in &message.content {
                match content {
                    ToolResultContent::Text(text) => {
                        lines.push(Line::from(format!("{}> {}", message.tool_name, text.text)))
                    }
                    ToolResultContent::Image(image) => lines.push(Line::from(format!(
                        "{}> [image: {}]",
                        message.tool_name, image.mime_type
                    ))),
                }
            }
        }
    }
}

impl Ui for Tui {
    fn wait_for_next_prompt(&mut self) -> io::Result<String> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        while state.input_responses.is_empty() && !state.shutdown {
            state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        Ok(if state.shutdown {
            "/exit".to_owned()
        } else {
            state.input_responses.pop_front().unwrap()
        })
    }

    fn inform(&mut self, title: &str, message: &str) {
        let mut state = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        state
            .notifications
            .push((title.to_owned(), message.to_owned()));
    }

    fn on_message(&mut self, message: &crate::core::Message) {
        let mut lines = Vec::new();
        append_message_lines(&mut lines, message);
        self.state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending_lines
            .extend(lines);
    }

    fn on_command(&mut self, command: &str) {
        self.state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending_lines
            .push(Line::from(format!("you> {command}")));
    }
}
