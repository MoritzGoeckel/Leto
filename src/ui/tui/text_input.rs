use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::AskOptions;

pub(super) const INPUT_PADDING: u16 = 2;

pub struct TextInput {
    text: String,
    cursor: usize,
    on_submit: Option<Box<dyn FnMut(&str) + Send>>,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            on_submit: None,
        }
    }

    pub fn wrapped_lines(
        &self,
        width: usize,
        options: &AskOptions,
    ) -> (Vec<Line<'static>>, usize, usize) {
        if self.text.is_empty() && !options.form_text.is_empty() {
            return (
                vec![Line::styled(
                    options.form_text.clone(),
                    options.form_text_color,
                )],
                0,
                0,
            );
        }
        let mut lines = vec![String::new()];
        let mut column = 0;
        let mut offset = 0;
        let mut cursor_position = (0, 0);
        for logical_line in self.text.split('\n') {
            let span = Span::raw(logical_line);
            for grapheme in span.styled_graphemes(Style::default()) {
                let symbol_width = Span::raw(grapheme.symbol).width();
                if column + symbol_width > width {
                    lines.push(String::new());
                    column = 0;
                }
                if offset == self.cursor {
                    cursor_position = (column, lines.len() - 1);
                }
                lines.last_mut().unwrap().push_str(grapheme.symbol);
                column += symbol_width;
                offset += grapheme.symbol.len();
            }
            if offset == self.cursor {
                cursor_position = if column == width {
                    (0, lines.len())
                } else {
                    (column, lines.len() - 1)
                };
                if column == width && offset == self.text.len() {
                    lines.push(String::new());
                }
            }
            if offset < self.text.len() {
                lines.push(String::new());
                column = 0;
                offset += 1;
            }
        }
        (
            lines.into_iter().map(Line::from).collect(),
            cursor_position.0,
            cursor_position.1,
        )
    }

    pub fn on_submit(&mut self, callback: impl FnMut(&str) + Send + 'static) {
        self.on_submit = Some(Box::new(callback));
    }

    pub fn insert_text(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('a') => self.cursor = 0,
                KeyCode::Char('e') => self.cursor = self.text.len(),
                KeyCode::Char('w') => self.delete_previous_word(),
                KeyCode::Left => self.move_cursor_word_left(),
                KeyCode::Right => self.move_cursor_word_right(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => self.insert_text("\n"),
            KeyCode::Enter => {
                if let Some(callback) = &mut self.on_submit {
                    callback(&self.text);
                }
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char(character) => self.insert_text(&character.to_string()),
            KeyCode::Backspace => self.delete_previous_character(),
            KeyCode::Delete => self.delete_next_character(),
            KeyCode::Left => self.move_cursor_left(),
            KeyCode::Right => self.move_cursor_right(),
            _ => {}
        }
    }

    fn move_cursor_left(&mut self) {
        if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
    }

    fn move_cursor_right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    fn move_cursor_word_left(&mut self) {
        let mut cursor = self.cursor;
        while let Some((index, character)) = self.text[..cursor].char_indices().next_back() {
            if !character.is_whitespace() {
                break;
            }
            cursor = index;
        }
        while let Some((index, character)) = self.text[..cursor].char_indices().next_back() {
            if character.is_whitespace() {
                break;
            }
            cursor = index;
        }
        self.cursor = cursor;
    }

    fn move_cursor_word_right(&mut self) {
        let mut cursor = self.cursor;
        while let Some(character) = self.text[cursor..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        while let Some(character) = self.text[cursor..].chars().next() {
            if character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        self.cursor = cursor;
    }

    fn delete_previous_character(&mut self) {
        if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.text.replace_range(index..self.cursor, "");
            self.cursor = index;
        }
    }

    fn delete_next_character(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            let end = self.cursor + character.len_utf8();
            self.text.replace_range(self.cursor..end, "");
        }
    }

    fn delete_previous_word(&mut self) {
        let mut start = self.cursor;
        for (index, character) in self.text[..self.cursor].char_indices().rev() {
            if character.is_whitespace() {
                if start < self.cursor {
                    break;
                }
            }
            start = index;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
}

pub(super) fn render_input(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    lines: Vec<Line<'static>>,
    cursor_column: usize,
    cursor_row: usize,
    options: &AskOptions,
) {
    let block = Block::default()
        .padding(Padding::horizontal(INPUT_PADDING))
        .style(Style::default().bg(options.background));
    let content_area = block.inner(area);
    let scroll = (cursor_row + 1).saturating_sub(area.height as usize);
    let input = Paragraph::new(lines)
        .scroll((scroll as u16, 0))
        .block(block)
        .style(
            Style::default()
                .bg(options.background)
                .fg(options.foreground),
        );
    frame.render_widget(input, area);
    frame.set_cursor_position((
        content_area.x + cursor_column as u16,
        content_area.y + (cursor_row - scroll) as u16,
    ));
}
