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
use text_input::{INPUT_BACKGROUND, INPUT_PADDING, TextInput, render_input};

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
    working: bool,
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
                viewport: Viewport::Inline(5),
            },
        )?;
        execute!(terminal.backend_mut(), EnableBracketedPaste)?;
        terminal.insert_before(1, |buffer| {
            Paragraph::new("").render(buffer.area, buffer);
        })?;
        let mut history_has_content = false;
        let result = loop {
            match self.tick(&mut terminal, &mut history_has_content) {
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
        history_has_content: &mut bool,
    ) -> io::Result<bool> {
        let (pending_lines, shutdown, notifications, working) = {
            let mut state = self.state.0.lock().unwrap();
            (
                std::mem::take(&mut state.pending_lines),
                state.shutdown,
                state.notifications.clone(),
                state.working,
            )
        };
        *history_has_content |= !pending_lines.is_empty();
        let size = terminal.size()?;
        let (input_lines, cursor_column, cursor_row) = self
            .text_input
            .lock()
            .unwrap()
            .wrapped_lines((size.width - 2 * INPUT_PADDING) as usize);
        let input_height = input_lines
            .len()
            .min(size.height.saturating_sub(4) as usize) as u16;
        let notification_height =
            notifications.len() + usize::from(*history_has_content && !notifications.is_empty());
        let working_separator_height =
            u16::from(working && (*history_has_content || !notifications.is_empty()));
        let working_height = u16::from(working);
        let viewport_height = (notification_height
            + input_height as usize
            + 4
            + working_separator_height as usize
            + working_height as usize)
            .min(size.height as usize) as u16;
        resize_viewport(terminal, viewport_height)?;
        if !pending_lines.is_empty() {
            let paragraph = Paragraph::new(pending_lines).wrap(Wrap { trim: false });
            let height = paragraph.line_count(size.width) as u16;
            terminal.insert_before(height, move |buffer| {
                paragraph.render(buffer.area, buffer);
            })?;
        }
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(working_separator_height),
                Constraint::Length(working_height),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
            render_notifications(frame, chunks[0], &notifications, *history_has_content);
            if working {
                frame.render_widget(Paragraph::new("Working..."), chunks[2]);
            }
            for separator in [chunks[4], chunks[6]] {
                frame.render_widget(
                    Paragraph::new("").style(Style::default().bg(INPUT_BACKGROUND)),
                    separator,
                );
            }
            render_input(frame, chunks[5], input_lines, cursor_column, cursor_row);
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

fn resize_viewport(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    height: u16,
) -> io::Result<()> {
    let area = terminal.get_frame().area();
    if height == area.height {
        return Ok(());
    }
    terminal.clear()?;
    let viewport_top = if height > area.height {
        let size = terminal.size()?;
        let scroll_height = (area.y + height).saturating_sub(size.height);
        execute!(
            terminal.backend_mut(),
            MoveTo(0, size.height - 1),
            Print("\r\n".repeat(scroll_height as usize)),
        )?;
        area.y - scroll_height
    } else {
        area.y
    };
    execute!(terminal.backend_mut(), MoveTo(0, viewport_top))?;
    *terminal = Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;
    terminal.clear()
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

    fn clear_notifications(&mut self) {
        self.state.0.lock().unwrap().notifications.clear();
    }

    fn start_working(&mut self) {
        self.state.0.lock().unwrap().working = true;
    }

    fn stop_working(&mut self) {
        self.state.0.lock().unwrap().working = false;
    }

    fn inform(&mut self, title: &str, message: &str) {
        let mut state = self.state.0.lock().unwrap();
        state
            .notifications
            .push((title.to_owned(), message.to_owned()));
    }

    fn note(&mut self, note: &str) {
        self.state
            .0
            .lock()
            .unwrap()
            .pending_lines
            .push(Line::from(note.to_owned()));
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
