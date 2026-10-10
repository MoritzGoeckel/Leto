use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
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
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::{AskOptions, INPUT_BACKGROUND, StrOptions, Ui};
mod text_input;
mod tools;
use text_input::{INPUT_PADDING, TextInput, render_input};

#[derive(Clone)]
pub struct Tui {
    state: Arc<(Mutex<State>, Condvar)>,
    text_input: Arc<Mutex<TextInput>>,
}

#[derive(Default)]
struct State {
    pending_lines: Vec<Line<'static>>,
    stream_text: String,
    history_has_content: bool,
    input_responses: VecDeque<String>,
    input_options: AskOptions,
    working_since: Option<Instant>,
    shutdown: bool,
    cancelled: bool,
    alert: Option<String>,
}

impl State {
    fn append_lines(&mut self, lines: Vec<Line<'static>>) {
        if lines.is_empty() {
            return;
        }
        if self.history_has_content {
            self.pending_lines.push(Line::default());
        }
        self.pending_lines.extend(lines);
        self.history_has_content = true;
    }
}

impl Tui {
    pub fn new() -> Self {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let mut text_input = TextInput::new();
        let submit_state = Arc::clone(&state);
        text_input.on_submit(move |text| {
            let (lock, wake) = &*submit_state;
            let mut state = lock.lock().unwrap();
            state.input_responses.push_back(text.to_owned());
            if state.working_since.is_some() {
                state.alert = Some(format!("Queueing prompts: {}", state.input_responses.len()));
            }
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
        let result = loop {
            match self.tick(&mut terminal) {
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

    fn tick(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<bool> {
        let (
            pending_lines,
            shutdown,
            working_since,
            input_options,
            history_has_content,
            alert,
            stream_text,
        ) = {
            let mut state = self.state.0.lock().unwrap();
            (
                std::mem::take(&mut state.pending_lines),
                state.shutdown,
                state.working_since,
                state.input_options.clone(),
                state.history_has_content,
                state.alert.clone(),
                state.stream_text.clone(),
            )
        };
        let size = terminal.size()?;
        let (input_lines, cursor_column, cursor_row) =
            self.text_input.lock().unwrap().wrapped_lines(
                size.width.saturating_sub(2 * INPUT_PADDING).max(1) as usize,
                &input_options,
            );
        let footer_height = 6;
        let input_height = input_lines
            .len()
            .min(size.height.saturating_sub(footer_height) as usize)
            as u16;
        let streaming = !stream_text.is_empty();
        let stream = Paragraph::new(if stream_text.is_empty() || !history_has_content {
            stream_text
        } else {
            format!("\n{stream_text}")
        })
        .wrap(Wrap { trim: false });
        let stream_lines = stream.line_count(size.width);
        let stream_height = if !streaming {
            0
        } else {
            stream_lines.min(size.height.saturating_sub(footer_height + input_height) as usize)
                as u16
        };
        let viewport_height = (footer_height + input_height + stream_height).min(size.height);
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
            let [
                preview,
                _,
                working,
                _,
                input_top,
                input,
                input_bottom,
                alert_row,
            ] = Layout::vertical([
                Constraint::Length(stream_height),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(area);
            frame.render_widget(
                stream.scroll((
                    stream_lines.saturating_sub(stream_height as usize) as u16,
                    0,
                )),
                preview,
            );
            if let Some(alert) = &alert {
                frame.render_widget(
                    Paragraph::new(format!("  {alert}")).style(Style::default().fg(Color::Yellow)),
                    alert_row,
                );
            }
            let status = if let Some(started) = working_since {
                let spinner =
                    ['/', '-', '\\', '|'][(started.elapsed().as_millis() / 150 % 4) as usize];
                Line::from(vec![
                    Span::styled("Working ", Style::default().fg(Color::DarkGray)),
                    Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow)),
                ])
            } else {
                Line::from(Span::styled("", Style::default().fg(Color::DarkGray)))
            };
            frame.render_widget(Paragraph::new(status), working);
            for separator in [input_top, input_bottom] {
                frame.render_widget(
                    Paragraph::new("").style(Style::default().bg(input_options.background)),
                    separator,
                );
            }
            render_input(
                frame,
                input,
                input_lines,
                cursor_column,
                cursor_row,
                &input_options,
            );
        })?;
        if shutdown {
            return Ok(true);
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if !self.handle_key_pressed(key) {
                        self.text_input.lock().unwrap().handle_key_event(key);
                    }
                }
                Event::Paste(text) => self.text_input.lock().unwrap().paste(&text),
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
            let mut state = self.state.0.lock().unwrap();
            if state.working_since.is_some() {
                state.cancelled = true;
                state.alert = Some("Stopping...".to_owned());
                return true;
            }
            return true;
        }
        false
    }
}

fn resize_viewport(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    height: u16,
) -> io::Result<()> {
    terminal.autoresize()?;
    let area = terminal.get_frame().area();
    if height == area.height {
        return Ok(());
    }
    terminal.clear()?;
    execute!(terminal.backend_mut(), MoveTo(0, area.y))?;
    *terminal = Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;
    terminal.clear()
}

fn user_message_lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|line| {
            Line::styled(
                line.to_owned(),
                Style::default().fg(Color::White).bg(INPUT_BACKGROUND),
            )
            .style(Style::default().fg(Color::White).bg(INPUT_BACKGROUND))
        })
        .collect()
}

fn append_message_lines(lines: &mut Vec<Line<'static>>, message: &crate::core::Message) {
    use crate::core::{AssistantContent, Message, SystemContent, UserContent, UserContentBlock};
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
            UserContent::Text(text) => lines.extend(user_message_lines(text)),
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        UserContentBlock::Text(text) => {
                            lines.extend(user_message_lines(&text.text))
                        }
                        UserContentBlock::Image(image) => lines
                            .extend(user_message_lines(&format!("[image: {}]", image.mime_type))),
                    }
                }
            }
        },
        Message::Assistant(message) => {
            for content in &message.content {
                match content {
                    AssistantContent::Text(text) => {
                        lines.extend(text.text.lines().map(|line| Line::from(line.to_owned())))
                    }
                    AssistantContent::Thinking(thinking) => lines.extend(
                        format!("thinking> {}", thinking.thinking)
                            .lines()
                            .map(|line| Line::from(line.to_owned())),
                    ),
                    AssistantContent::ToolCall(call) => tools::append_call(lines, call),
                }
            }
        }
        Message::ToolResult(message) => tools::append_result(lines, message),
    }
}

impl Ui for Tui {
    fn ask_styled(&mut self, options: AskOptions) -> io::Result<String> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.input_options = options;
        while state.input_responses.is_empty() && !state.shutdown {
            state = wake.wait(state).unwrap();
        }
        state.input_options = AskOptions::default();
        if state.shutdown {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let input = state.input_responses.pop_front().unwrap();
        state.alert = None;
        Ok(std::iter::once(input)
            .chain(state.input_responses.drain(..))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn take_cancel(&mut self) -> bool {
        std::mem::take(&mut self.state.0.lock().unwrap().cancelled)
    }

    fn start_working(&mut self) {
        let mut state = self.state.0.lock().unwrap();
        state.working_since = Some(Instant::now());
    }

    fn stop_working(&mut self) {
        let mut state = self.state.0.lock().unwrap();
        state.working_since = None;
        state.alert = None;
    }

    fn set_alert(&mut self, message: &str) {
        self.state.0.lock().unwrap().alert = Some(message.to_owned());
    }

    fn clear_alert(&mut self) {
        self.state.0.lock().unwrap().alert = None;
    }

    fn append_message_str(&mut self, note: &str) {
        self.append_message_str_styled(
            note,
            StrOptions {
                background: Color::Reset,
                foreground: Color::Reset,
            },
        );
    }

    fn append_message_str_styled(&mut self, note: &str, options: StrOptions) {
        self.state.0.lock().unwrap().append_lines(
            note.lines()
                .map(|line| {
                    Line::styled(
                        line.to_owned(),
                        Style::default()
                            .fg(options.foreground)
                            .bg(options.background),
                    )
                })
                .collect(),
        );
    }

    fn append_message(&mut self, message: &crate::core::Message) {
        let mut lines = Vec::new();
        append_message_lines(&mut lines, message);
        let mut state = self.state.0.lock().unwrap();
        if matches!(message, crate::core::Message::Assistant(_)) {
            state.stream_text.clear();
        }
        state.append_lines(lines);
    }

    fn append_stream_delta(&mut self, delta: &str) {
        self.state.0.lock().unwrap().stream_text.push_str(delta);
    }

    fn append_command(&mut self, command: &str) {
        self.state.0.lock().unwrap().append_lines(vec![Line::styled(
            command.to_owned(),
            Style::default().fg(Color::Cyan),
        )]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_stream_enters_history_once_with_line_breaks() {
        let mut tui = Tui::new();
        tui.append_message_str("history");
        tui.append_stream_delta("first\n");
        tui.append_stream_delta("second");
        {
            let state = tui.state.0.lock().unwrap();
            assert_eq!(state.pending_lines.len(), 1);
            assert_eq!(state.pending_lines[0].to_string(), "history");
            assert_eq!(state.stream_text, "first\nsecond");
        }
        let message = serde_json::from_value(serde_json::json!({
            "content": [{"type": "text", "text": "first\nsecond"}],
            "api": "test", "provider": "test", "model": "test",
            "usage": crate::core::Usage::default(), "stopReason": "stop", "timestamp": 0
        }))
        .unwrap();
        tui.append_message(&crate::core::Message::Assistant(message));
        let state = tui.state.0.lock().unwrap();
        assert!(state.stream_text.is_empty());
        assert_eq!(
            state
                .pending_lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["history", "", "first", "second"]
        );
    }
}
