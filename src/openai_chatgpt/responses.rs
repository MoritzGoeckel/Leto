use crate::core::{
    AssistantContent, AssistantMessage, CompletionReason, Context, Model, StopReason,
    StreamOptions, TextContent, ThinkingContent, ToolCall, Usage,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

pub fn stream(
    model: &Model,
    context: &Context,
    options: &StreamOptions,
) -> Result<Vec<crate::core::AssistantMessageEvent>, Box<dyn std::error::Error>> {
    let token = options
        .api_key
        .as_deref()
        .ok_or("missing OpenAI access token")?;
    let mut input = Vec::new();
    for message in &context.messages {
        match message {
            crate::core::Message::System(message) => input.push(json!({"role":"system","content":system_text(&message.content)})),
            crate::core::Message::User(message) => input.push(json!({"role":"user","content":user_content(&message.content)})),
            crate::core::Message::Assistant(message) => for content in &message.content {
                match content {
                    AssistantContent::Text(text) => input.push(json!({"role":"assistant","content":text.text})),
                    AssistantContent::Thinking(thinking) => input.push(json!({"role":"assistant","content":thinking.thinking})),
                    AssistantContent::ToolCall(call) => input.push(json!({"type":"function_call","call_id":call.id,"name":call.name,"arguments":serde_json::to_string(&call.arguments)?})),
                }
            },
            crate::core::Message::ToolResult(result) => input.push(json!({"type":"function_call_output","call_id":result.tool_call_id,"output":result.content.iter().filter_map(|block| match block { crate::core::ToolResultContent::Text(text) => Some(text.text.as_str()), _ => None }).collect::<Vec<_>>().join("\n")})),
        }
    }
    if let Some(prompt) = &context.system_prompt {
        input.insert(0, json!({"role":"system","content":prompt}));
    }
    let mut body = json!({"model":model.id,"input":input,"stream":true,"store":false});
    if model.reasoning {
        body["reasoning"] = json!({"effort":"none"});
    }
    if !token.starts_with("sk-") { /* ChatGPT OAuth rejects tuning and cache fields. */
    } else {
        if let Some(max_tokens) = options.max_tokens {
            body["max_output_tokens"] = json!(max_tokens.max(16));
        }
        if let Some(temperature) = options.temperature {
            body["temperature"] = json!(temperature);
        }
    }
    if let Some(tools) = &context.tools {
        body["tools"] = json!(tools.iter().map(|tool| json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.parameters})).collect::<Vec<_>>());
    }
    if let Some(params) = &options.sampling_params {
        for (key, value) in params {
            body[key] = value.clone();
        }
    }
    let response = Client::new()
        .post(format!(
            "{}/responses",
            model.base_url.trim_end_matches('/')
        ))
        .bearer_auth(token)
        .header("user-agent", "atlas (rust)")
        .header("accept", "text/event-stream")
        .json(&body)
        .send()?;
    let status = response.status();
    // TODO: Parse SSE incrementally; use /root/repos/pi/packages/ai/src/api/openai-responses-shared.ts as the reference.
    let data = response.text()?;
    if !status.is_success() {
        return Err(format!("OpenAI Responses API returned {status}: {data}").into());
    }
    let mut text = String::new();
    let mut thinking = String::new();
    let mut calls = Vec::new();
    let mut usage = Usage::default();
    let mut stop = StopReason::Stop;
    for line in data.lines().filter_map(|line| line.strip_prefix("data: ")) {
        if line == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(line)?;
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => text.push_str(event["delta"].as_str().unwrap_or("")),
            "response.reasoning_summary_text.delta" => {
                thinking.push_str(event["delta"].as_str().unwrap_or(""))
            }
            "response.output_item.done" if event["item"]["type"] == "function_call" => {
                let item = &event["item"];
                calls.push(ToolCall {
                    id: item["call_id"].as_str().unwrap_or_default().to_owned(),
                    name: item["name"].as_str().unwrap_or_default().to_owned(),
                    arguments: serde_json::from_str(item["arguments"].as_str().unwrap_or("{}"))?,
                    thought_signature: None,
                    namespace: None,
                });
            }
            "response.completed" => {
                usage.input = event["response"]["usage"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                usage.output = event["response"]["usage"]["output_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                usage.total_tokens = usage.input + usage.output;
                if event["response"]["status"] == "incomplete" {
                    stop = StopReason::Length;
                }
            }
            "error" | "response.failed" => return Err(event.to_string().into()),
            _ => {}
        }
    }
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(AssistantContent::Text(TextContent {
            text,
            text_signature: None,
        }));
    }
    if !thinking.is_empty() {
        content.push(AssistantContent::Thinking(ThinkingContent {
            thinking,
            thinking_signature: None,
            redacted: None,
        }));
    }
    content.extend(calls.iter().cloned().map(AssistantContent::ToolCall));
    if !calls.is_empty() {
        stop = StopReason::ToolUse;
    }
    let message = AssistantMessage {
        content,
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        provider_thinking_level: None,
        thinking_level: options.reasoning.clone(),
        diagnostics: None,
        usage,
        stop_reason: stop.clone(),
        deferred: None,
        error_message: None,
        raw_stop_reason: None,
        end_turn: None,
        timestamp: now_ms(),
    };
    let reason = match stop {
        StopReason::Length => CompletionReason::Length,
        StopReason::ToolUse => CompletionReason::ToolUse,
        _ => CompletionReason::Stop,
    };
    Ok(vec![
        crate::core::AssistantMessageEvent::Start {
            partial: message.clone(),
        },
        crate::core::AssistantMessageEvent::Done { reason, message },
    ])
}

fn system_text(content: &crate::core::SystemContent) -> String {
    match content {
        crate::core::SystemContent::Text(text) => text.clone(),
        crate::core::SystemContent::Blocks(blocks) => blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}
fn user_content(content: &crate::core::UserContent) -> Value {
    match content { crate::core::UserContent::Text(text) => json!([{ "type":"input_text", "text":text }]), crate::core::UserContent::Blocks(blocks) => json!(blocks.iter().map(|block| match block { crate::core::UserContentBlock::Text(text) => json!({"type":"input_text","text":text.text}), crate::core::UserContentBlock::Image(image) => json!({"type":"input_image","image_url":format!("data:{};base64,{}",image.mime_type,image.data),"detail":"auto"}) }).collect::<Vec<_>>()) }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
