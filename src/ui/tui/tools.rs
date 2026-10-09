use ratatui::{
    style::{Color, Style},
    text::Line,
};
use serde_json::Value;

use crate::core::{ToolCall, ToolResultContent, ToolResultMessage};

pub(super) fn append_call(lines: &mut Vec<Line<'static>>, call: &ToolCall) {
    match call.name.as_str() {
        "bash" => bash_call(lines, &call.arguments),
        _ => append_body(
            lines,
            &serde_json::to_string_pretty(&call.arguments).unwrap(),
            Color::DarkGray,
        ),
    }
}

pub(super) fn append_result(lines: &mut Vec<Line<'static>>, message: &ToolResultMessage) {
    for content in &message.content {
        let body = match (message.tool_name.as_str(), content) {
            ("bash", ToolResultContent::Json(result)) => bash_result(result),
            ("bash", ToolResultContent::Text(text)) => text.text.clone(),
            (_, ToolResultContent::Text(text)) => text.text.clone(),
            (_, ToolResultContent::Json(value)) => value.to_string(),
            (_, ToolResultContent::Image(image)) => format!("[image: {}]", image.mime_type),
        };
        let color = if message.is_error {
            Color::Red
        } else {
            Color::DarkGray
        };
        append_result_body(lines, &body, color);
    }
}

fn bash_call(lines: &mut Vec<Line<'static>>, arguments: &serde_json::Map<String, Value>) {
    append_body(
        lines,
        &format!("BASH: {}", arguments["command"].as_str().unwrap()),
        Color::Cyan,
    );
}

fn bash_result(result: &Value) -> String {
    let output = result["output"].as_str().unwrap();
    if result["truncated"].as_bool().unwrap() {
        format!(
            "{output}\n[Output truncated. Full output: {}]",
            result["full_output_path"].as_str().unwrap()
        )
    } else {
        output.to_owned()
    }
}

fn append_result_body(lines: &mut Vec<Line<'static>>, text: &str, color: Color) {
    let characters: Vec<char> = text.chars().collect();
    if characters.len() <= 500 {
        append_body(lines, text, color);
        return;
    }
    append_body(lines, &characters[..250].iter().collect::<String>(), color);
    lines.push(Line::styled("...", Style::default().fg(Color::Yellow)));
    append_body(
        lines,
        &characters[characters.len() - 250..]
            .iter()
            .collect::<String>(),
        color,
    );
}

fn append_body(lines: &mut Vec<Line<'static>>, text: &str, color: Color) {
    lines.extend(
        text.split('\n')
            .map(|line| Line::styled(line.to_owned(), Style::default().fg(color))),
    );
}
