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
    history_has_content: bool,
    input_responses: VecDeque<String>,
    input_options: AskOptions,
    working_since: Option<Instant>,
    shutdown: bool,
    cancelled: bool,
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
        let (pending_lines, shutdown, working_since, input_options, history_has_content) = {
            let mut state = self.state.0.lock().unwrap();
            (
                std::mem::take(&mut state.pending_lines),
                state.shutdown,
                state.working_since,
                state.input_options.clone(),
                state.history_has_content,
            )
        };
        let working = working_since.is_some();
        let size = terminal.size()?;
        let (input_lines, cursor_column, cursor_row) = self
            .text_input
            .lock()
            .unwrap()
            .wrapped_lines((size.width - 2 * INPUT_PADDING) as usize, &input_options);
        let input_height = input_lines
            .len()
            .min(size.height.saturating_sub(4) as usize) as u16;
        let working_separator_height = u16::from(working && history_has_content);
        let working_height = u16::from(working);
        let viewport_height = (input_height as usize
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
            if let Some(started) = working_since {
                let spinner =
                    ['/', '-', '\\', '|'][(started.elapsed().as_millis() / 150 % 4) as usize];
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("Working ", Style::default().fg(Color::DarkGray)),
                        Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow)),
                    ])),
                    chunks[2],
                );
            }
            for separator in [chunks[4], chunks[6]] {
                frame.render_widget(
                    Paragraph::new("").style(Style::default().bg(input_options.background)),
                    separator,
                );
            }
            render_input(
                frame,
                chunks[5],
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
            let mut state = lock.lock().unwrap();
            if state.working_since.is_some() {
                state.cancelled = true;
                return true;
            }
            state.input_responses.push_back(String::new());
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
                        lines.push(Line::from(format!("{}", text.text)))
                    }
                    AssistantContent::Thinking(thinking) => {
                        lines.push(Line::from(format!("thinking> {}", thinking.thinking)))
                    }
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
        Ok(std::iter::once(input)
            .chain(state.input_responses.drain(..))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn take_cancel(&mut self) -> bool {
        std::mem::take(&mut self.state.0.lock().unwrap().cancelled)
    }

    fn start_working(&mut self) {
        self.state.0.lock().unwrap().working_since = Some(Instant::now());
    }

    fn stop_working(&mut self) {
        self.state.0.lock().unwrap().working_since = None;
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
        self.state.0.lock().unwrap().append_lines(lines);
    }

    fn append_command(&mut self, command: &str) {
        self.state.0.lock().unwrap().append_lines(vec![Line::styled(
            command.to_owned(),
            Style::default().fg(Color::Cyan),
        )]);
    }
}
