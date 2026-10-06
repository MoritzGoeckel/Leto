use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use crossterm::{
    cursor::MoveTo,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    },
    execute,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::Ui;
mod text_input;
use text_input::TextInput;

#[derive(Clone)]
pub struct Tui {
    state: Arc<(Mutex<State>, Condvar)>,
    text_input: Arc<Mutex<TextInput>>,
}

#[derive(Default)]
struct State {
    pending_lines: Vec<Line<'static>>,
    input_responses: VecDeque<String>,
    notifications: Vec<(String, String)>,
    shutdown: bool,
}

impl Tui {
    pub fn new() -> Self {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let mut text_input = TextInput::new();
        let submit_state = Arc::clone(&state);
        text_input.on_submit(move |text| {
            let (lock, wake) = &*submit_state;
            lock.lock()
                .unwrap()
                .input_responses
                .push_back(text.to_owned());
            wake.notify_all();
        });
        Self {
            state,
            text_input: Arc::new(Mutex::new(text_input)),
        }
    }

    pub fn stop(&self) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().shutdown = true;
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
        execute!(terminal.backend_mut(), EnableBracketedPaste)?;
        terminal.insert_before(1, |buffer| {
            Paragraph::new("").render(buffer.area, buffer);
        })?;
        let mut viewport_height = 3;
        let mut history_has_content = false;
        let result = loop {
            match self.tick(
                &mut terminal,
                &mut viewport_height,
                &mut history_has_content,
            ) {
                Ok(true) => break Ok(()),
                Ok(false) => {}
                Err(error) => break Err(error),
            }
        };
        execute!(terminal.backend_mut(), DisableBracketedPaste)?;
        disable_raw_mode()?;
        let last_row = terminal.size()?.height - 1;
        execute!(terminal.backend_mut(), MoveTo(0, last_row), Print("\r\n"))?;
        terminal.show_cursor()?;
        result
    }

    fn tick(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        viewport_height: &mut u16,
        history_has_content: &mut bool,
    ) -> io::Result<bool> {
        let (pending_lines, shutdown, notification_count) = {
            let mut state = self.state.0.lock().unwrap();
            (
                std::mem::take(&mut state.pending_lines),
                state.shutdown,
                state.notifications.len(),
            )
        };
        *history_has_content |= !pending_lines.is_empty();
        let next_height = notification_count as u16
            + 3
            + u16::from(*history_has_content && notification_count > 0);
        if next_height > *viewport_height {
            let added_rows = next_height - *viewport_height;
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
            *viewport_height = next_height;
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
        let notifications = self.state.0.lock().unwrap().notifications.clone();
        let (input_text, cursor_column) = {
            let text_input = self.text_input.lock().unwrap();
            (text_input.text().to_owned(), text_input.cursor_column())
        };
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
            render_notifications(frame, chunks[0], &notifications, *history_has_content);
            render_input(frame, chunks[2], &input_text, cursor_column);
        })?;
        if shutdown {
            return Ok(true);
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if self.handle_key_pressed(key) {
                        return Ok(true);
                    }
                    self.text_input.lock().unwrap().handle_key_event(key);
                }
                Event::Paste(text) => self.text_input.lock().unwrap().insert_text(&text),
                _ => {}
            }
        }
        Ok(false)
    }

    fn handle_key_pressed(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
            self.stop();
            return true;
        }
        if key.code == KeyCode::Esc {
            let (lock, wake) = &*self.state;
            lock.lock()
                .unwrap()
                .input_responses
                .push_back(String::new());
            wake.notify_all();
            return true;
        }
        false
    }
}

fn render_notifications(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    notifications: &[(String, String)],
    history_has_content: bool,
) {
    let mut lines: Vec<_> = notifications
        .iter()
        .map(|(title, notice)| {
            Line::from(Span::styled(
                format!("{title}: {notice}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    if history_has_content && !lines.is_empty() {
        lines.insert(0, Line::default());
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_input(frame: &mut ratatui::Frame<'_>, area: Rect, text: &str, cursor_column: usize) {
    let input = Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(input, area);
    frame.set_cursor_position((area.x.saturating_add(cursor_column as u16), area.y));
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
        let mut state = lock.lock().unwrap();
        while state.input_responses.is_empty() && !state.shutdown {
            state = wake.wait(state).unwrap();
        }
        Ok(if state.shutdown {
            "/exit".to_owned()
        } else {
            state.input_responses.pop_front().unwrap()
        })
    }

    fn inform(&mut self, title: &str, message: &str) {
        let mut state = self.state.0.lock().unwrap();
        state
            .notifications
            .push((title.to_owned(), message.to_owned()));
    }

    fn on_message(&mut self, message: &crate::core::Message) {
        let mut lines = Vec::new();
        append_message_lines(&mut lines, message);
        self.state.0.lock().unwrap().pending_lines.extend(lines);
    }

    fn on_command(&mut self, command: &str) {
        self.state
            .0
            .lock()
            .unwrap()
            .pending_lines
            .push(Line::from(format!("you> {command}")));
    }
}
