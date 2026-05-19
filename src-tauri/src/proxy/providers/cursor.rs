//! Cursor proxy protocol adapters.
//!
//! Cursor speaks OpenAI-compatible `/v1/chat/completions` for many custom
//! models and `/v1/responses` for others. The local proxy stores rich provider
//! config under the Claude namespace, so these helpers bridge Cursor requests
//! to Anthropic Messages first. The existing Claude provider adapter can then
//! fan that Anthropic-shaped request out to native Anthropic, OpenAI Chat,
//! OpenAI Responses, Gemini Native, Copilot, or Codex OAuth upstreams.

use crate::proxy::error::ProxyError;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

fn gen_id(prefix: &str) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if let Some(text) = part.as_str() {
                    Some(text.to_string())
                } else if part.get("type").and_then(Value::as_str) == Some("text") {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn content_to_anthropic_blocks(content: Option<&Value>) -> Vec<Value> {
    let Some(content) = content else {
        return Vec::new();
    };

    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "text", "text": text })]
            }
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if let Some(text) = part.as_str() {
                    return Some(json!({ "type": "text", "text": text }));
                }

                let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
                match part_type {
                    "text" => Some(json!({
                        "type": "text",
                        "text": part.get("text").and_then(Value::as_str).unwrap_or("")
                    })),
                    "image" | "tool_use" | "tool_result" => Some(part.clone()),
                    "image_url" => {
                        let url_value = part.get("image_url").unwrap_or(part);
                        let url = url_value
                            .get("url")
                            .and_then(Value::as_str)
                            .or_else(|| url_value.as_str())
                            .unwrap_or("");
                        if let Some(rest) = url.strip_prefix("data:") {
                            let (media_type, data) =
                                rest.split_once(";base64,").unwrap_or(("image/png", ""));
                            Some(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": data
                                }
                            }))
                        } else {
                            Some(json!({
                                "type": "image",
                                "source": { "type": "url", "url": url }
                            }))
                        }
                    }
                    _ => None,
                }
            })
            .collect(),
        other if !other.is_null() => vec![json!({ "type": "text", "text": other.to_string() })],
        _ => Vec::new(),
    }
}

fn parse_tool_arguments(arguments: Option<&Value>) -> Value {
    match arguments {
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or_else(|_| json!({})),
        Some(Value::Object(_)) | Some(Value::Array(_)) => arguments.cloned().unwrap_or(json!({})),
        _ => json!({}),
    }
}

fn merge_same_role(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        if let Some(last) = merged.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(role.as_str()) {
                let mut last_blocks = content_to_anthropic_blocks(last.get("content"));
                let mut next_blocks = content_to_anthropic_blocks(message.get("content"));
                last_blocks.append(&mut next_blocks);
                last["content"] = Value::Array(last_blocks);
                continue;
            }
        }
        merged.push(message);
    }

    merged
}

fn convert_tools_to_anthropic(tools: Option<&Value>) -> Vec<Value> {
    tools
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    if tool.get("type").and_then(Value::as_str) == Some("function") {
                        let function = tool.get("function")?;
                        Some(json!({
                            "name": function.get("name").and_then(Value::as_str).unwrap_or(""),
                            "description": function.get("description").cloned().unwrap_or(json!("")),
                            "input_schema": function.get("parameters").cloned().unwrap_or(json!({
                                "type": "object",
                                "properties": {}
                            }))
                        }))
                    } else if tool.get("name").is_some() && tool.get("input_schema").is_some() {
                        Some(json!({
                            "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                            "description": tool.get("description").cloned().unwrap_or(json!("")),
                            "input_schema": tool.get("input_schema").cloned().unwrap_or(json!({
                                "type": "object",
                                "properties": {}
                            }))
                        }))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn append_chat_message_as_anthropic(
    message: &Value,
    system_parts: &mut Vec<String>,
    out: &mut Vec<Value>,
) {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let content = message.get("content");

    if role == "system" {
        let text = content.map(value_to_text).unwrap_or_default();
        if !text.is_empty() {
            system_parts.push(text);
        }
        return;
    }

    if role == "tool" {
        let text = content.map(value_to_text).unwrap_or_default();
        out.push(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": message.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                "content": text
            }]
        }));
        return;
    }

    let anthropic_role = if role == "assistant" {
        "assistant"
    } else {
        "user"
    };
    let mut blocks = content_to_anthropic_blocks(content);

    if role == "assistant" {
        if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
            if !reasoning.is_empty() {
                blocks.insert(0, json!({ "type": "thinking", "thinking": reasoning }));
            }
        }

        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                blocks.push(json!({
                    "type": "tool_use",
                    "id": tool_call.get("id").and_then(Value::as_str).unwrap_or("toolu_"),
                    "name": tool_call.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                    "input": parse_tool_arguments(tool_call.pointer("/function/arguments"))
                }));
            }
        }
    }

    if !blocks.is_empty() {
        out.push(json!({ "role": anthropic_role, "content": blocks }));
    }
}

/// Convert OpenAI Chat Completions request to Anthropic Messages request.
pub fn chat_to_anthropic_request(payload: Value) -> Result<Value, ProxyError> {
    let mut system_parts = Vec::new();
    let mut anthropic_messages = Vec::new();

    for message in payload
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        append_chat_message_as_anthropic(&message, &mut system_parts, &mut anthropic_messages);
    }

    let mut result = json!({
        "model": payload.get("model").cloned().unwrap_or(json!("claude-sonnet-4-5")),
        "messages": merge_same_role(anthropic_messages),
        "max_tokens": payload
            .get("max_tokens")
            .cloned()
            .unwrap_or(json!(8192))
    });

    if let Some(stream) = payload.get("stream") {
        result["stream"] = stream.clone();
    }
    if !system_parts.is_empty() {
        result["system"] = json!(system_parts.join("\n\n"));
    }
    let tools = convert_tools_to_anthropic(payload.get("tools"));
    if !tools.is_empty() {
        result["tools"] = json!(tools);
    }
    for key in ["temperature", "top_p"] {
        if let Some(value) = payload.get(key) {
            result[key] = value.clone();
        }
    }

    Ok(result)
}

fn response_input_to_chat_messages(input: &Value) -> Vec<Value> {
    if let Some(text) = input.as_str() {
        return vec![json!({ "role": "user", "content": text })];
    }

    let Some(items) = input.as_array() else {
        return Vec::new();
    };

    let mut messages = Vec::new();
    for item in items {
        if let Some(text) = item.as_str() {
            messages.push(json!({ "role": "user", "content": text }));
            continue;
        }

        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "message" => {
                messages.push(json!({
                    "role": item.get("role").and_then(Value::as_str).unwrap_or("user"),
                    "content": item.get("content").map(value_to_text).unwrap_or_default()
                }));
            }
            "function_call" => {
                messages.push(json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": item.get("call_id").and_then(Value::as_str).unwrap_or("call_"),
                        "type": "function",
                        "function": {
                            "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
                            "arguments": item.get("arguments").and_then(Value::as_str).unwrap_or("{}")
                        }
                    }]
                }));
            }
            "function_call_output" => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                    "content": item.get("output").cloned().unwrap_or(json!("")).to_string()
                }));
            }
            _ if item.get("role").is_some() => {
                messages.push(json!({
                    "role": item.get("role").and_then(Value::as_str).unwrap_or("user"),
                    "content": item.get("content").map(value_to_text).unwrap_or_default()
                }));
            }
            _ => {}
        }
    }
    messages
}

/// Convert OpenAI Responses request to Anthropic Messages request.
pub fn responses_to_anthropic_request(payload: Value) -> Result<Value, ProxyError> {
    let mut chat = json!({
        "model": payload.get("model").cloned().unwrap_or(json!("")),
        "messages": payload
            .get("input")
            .map(response_input_to_chat_messages)
            .unwrap_or_default(),
        "stream": payload.get("stream").cloned().unwrap_or(json!(false))
    });

    if let Some(instructions) = payload.get("instructions").and_then(Value::as_str) {
        if !instructions.is_empty() {
            let mut messages = chat
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            messages.insert(0, json!({ "role": "system", "content": instructions }));
            chat["messages"] = json!(messages);
        }
    }
    if let Some(max) = payload.get("max_output_tokens") {
        chat["max_tokens"] = max.clone();
    }
    for key in ["tools", "temperature", "top_p", "tool_choice"] {
        if let Some(value) = payload.get(key) {
            chat[key] = value.clone();
        }
    }

    chat_to_anthropic_request(chat)
}

fn anthropic_stop_to_chat(stop: Option<&str>) -> &'static str {
    match stop {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        Some("stop_sequence") | Some("end_turn") | None => "stop",
        Some(_) => "stop",
    }
}

/// Convert Anthropic Messages response to OpenAI Chat Completions response.
pub fn anthropic_to_chat_response(
    data: Value,
    request_id: Option<String>,
) -> Result<Value, ProxyError> {
    let mut content_text = String::new();
    let mut reasoning_text = String::new();
    let mut tool_calls = Vec::new();

    for block in data
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    content_text.push_str(text);
                }
            }
            "thinking" => {
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    reasoning_text.push_str(text);
                }
            }
            "tool_use" => {
                let input = block.get("input").cloned().unwrap_or(json!({}));
                let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(json!({
                    "index": tool_calls.len(),
                    "id": block.get("id").and_then(Value::as_str).unwrap_or("toolu_"),
                    "type": "function",
                    "function": {
                        "name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                        "arguments": arguments
                    }
                }));
            }
            _ => {}
        }
    }

    let usage = data.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut message = json!({
        "role": "assistant",
        "content": if content_text.is_empty() { Value::Null } else { json!(content_text) }
    });
    if !reasoning_text.is_empty() {
        message["reasoning_content"] = json!(reasoning_text);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }

    Ok(json!({
        "id": request_id.unwrap_or_else(|| gen_id("chatcmpl-")),
        "object": "chat.completion",
        "model": data.get("model").and_then(Value::as_str).unwrap_or(""),
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": anthropic_stop_to_chat(data.get("stop_reason").and_then(Value::as_str))
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    }))
}

/// Convert Chat Completions response to OpenAI Responses response.
pub fn chat_to_responses_response(chat: Value, model: &str) -> Result<Value, ProxyError> {
    let choice = chat
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .cloned()
        .unwrap_or(json!({}));
    let message = choice.get("message").cloned().unwrap_or(json!({}));
    let finish = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");

    let mut output = Vec::new();
    if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
        if !reasoning.is_empty() {
            output.push(json!({
                "type": "reasoning",
                "id": gen_id("rs_"),
                "summary": [{ "type": "summary_text", "text": reasoning }]
            }));
        }
    }
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            output.push(json!({
                "type": "message",
                "id": gen_id("msg_"),
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }]
            }));
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            output.push(json!({
                "type": "function_call",
                "id": gen_id("fc_"),
                "status": "completed",
                "call_id": tool_call.get("id").and_then(Value::as_str).unwrap_or("call_"),
                "name": tool_call.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                "arguments": tool_call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}")
            }));
        }
    }

    let usage = chat.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Ok(json!({
        "id": chat.get("id").and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| gen_id("resp_")),
        "object": "response",
        "status": if finish == "length" { "incomplete" } else { "completed" },
        "model": if model.is_empty() { chat.get("model").and_then(Value::as_str).unwrap_or("") } else { model },
        "output": output,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": usage
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(input_tokens + output_tokens)
        }
    }))
}

/// Convert Anthropic Messages response to OpenAI Responses response.
pub fn anthropic_to_responses_response(data: Value, model: &str) -> Result<Value, ProxyError> {
    chat_to_responses_response(anthropic_to_chat_response(data, None)?, model)
}

fn sse_json(event: &str, data: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_default()
    ))
}

fn chat_chunk(
    id: &str,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Value {
    let mut choice = json!({ "index": 0, "delta": delta });
    if let Some(reason) = finish_reason {
        choice["finish_reason"] = json!(reason);
    }
    let mut chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [choice]
    });
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    chunk
}

fn chat_sse_data(chunk: &Value) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        serde_json::to_string(chunk).unwrap_or_default()
    ))
}

/// Convert Anthropic SSE to OpenAI Chat Completions SSE.
pub fn anthropic_sse_to_chat<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    request_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let id = gen_id("chatcmpl-");
        let mut model = request_model;
        let mut tool_index: i64 = -1;
        let mut input_tokens = 0_u64;

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    continue;
                }
            };
            crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

            while let Some(block) = take_sse_block(&mut buffer) {
                let mut event_name = "";
                let mut data_lines = Vec::new();
                for line in block.lines() {
                    if let Some(event) = strip_sse_field(line, "event") {
                        event_name = event.trim();
                    } else if let Some(data) = strip_sse_field(line, "data") {
                        data_lines.push(data);
                    }
                }
                if data_lines.is_empty() {
                    continue;
                }
                let data = data_lines.join("\n");
                if data.trim() == "[DONE]" {
                    continue;
                }
                let value: Value = match serde_json::from_str(&data) {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                match event_name {
                    "message_start" => {
                        if let Some(upstream_model) = value.pointer("/message/model").and_then(Value::as_str) {
                            model = upstream_model.to_string();
                        }
                        input_tokens = value.pointer("/message/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let chunk = chat_chunk(&id, &model, json!({"role":"assistant","content":""}), None, None);
                        yield Ok(chat_sse_data(&chunk));
                    }
                    "content_block_start"
                        if value.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") =>
                    {
                            tool_index += 1;
                            let chunk = chat_chunk(&id, &model, json!({
                                "tool_calls": [{
                                    "index": tool_index,
                                    "id": value.pointer("/content_block/id").and_then(Value::as_str).unwrap_or("toolu_"),
                                    "type": "function",
                                    "function": {
                                        "name": value.pointer("/content_block/name").and_then(Value::as_str).unwrap_or(""),
                                        "arguments": ""
                                    }
                                }]
                            }), None, None);
                            yield Ok(chat_sse_data(&chunk));
                    }
                    "content_block_delta" => {
                        match value.pointer("/delta/type").and_then(Value::as_str).unwrap_or("") {
                            "text_delta" => {
                                if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str) {
                                    let chunk = chat_chunk(&id, &model, json!({"content": text}), None, None);
                                    yield Ok(chat_sse_data(&chunk));
                                }
                            }
                            "thinking_delta" => {
                                if let Some(text) = value.pointer("/delta/thinking").and_then(Value::as_str) {
                                    let chunk = chat_chunk(&id, &model, json!({"reasoning_content": text}), None, None);
                                    yield Ok(chat_sse_data(&chunk));
                                }
                            }
                            "input_json_delta" => {
                                if let Some(partial) = value.pointer("/delta/partial_json").and_then(Value::as_str) {
                                    let chunk = chat_chunk(&id, &model, json!({
                                        "tool_calls": [{
                                            "index": tool_index.max(0),
                                            "function": { "arguments": partial }
                                        }]
                                    }), None, None);
                                    yield Ok(chat_sse_data(&chunk));
                                }
                            }
                            _ => {}
                        }
                    }
                    "message_delta" => {
                        let output_tokens = value.pointer("/usage/output_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let usage = json!({
                            "prompt_tokens": input_tokens,
                            "completion_tokens": output_tokens,
                            "total_tokens": input_tokens + output_tokens
                        });
                        let finish = anthropic_stop_to_chat(value.pointer("/delta/stop_reason").and_then(Value::as_str));
                        let chunk = chat_chunk(&id, &model, json!({}), Some(finish), Some(usage));
                        yield Ok(chat_sse_data(&chunk));
                    }
                    _ => {}
                }
            }
        }
        yield Ok(Bytes::from("data: [DONE]\n\n"));
    }
}

/// Convert OpenAI Responses SSE to OpenAI Chat Completions SSE.
pub fn responses_sse_to_chat<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    request_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let id = gen_id("chatcmpl-");
        let mut model = request_model;
        let mut next_tool_index = 0_i64;
        let mut tool_slots: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    continue;
                }
            };
            crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
            while let Some(block) = take_sse_block(&mut buffer) {
                let mut event_name = "";
                let mut data_lines = Vec::new();
                for line in block.lines() {
                    if let Some(event) = strip_sse_field(line, "event") {
                        event_name = event.trim();
                    } else if let Some(data) = strip_sse_field(line, "data") {
                        data_lines.push(data);
                    }
                }
                let data = data_lines.join("\n");
                let value: Value = match serde_json::from_str(&data) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                match event_name {
                    "response.created" => {
                        if let Some(upstream_model) = value
                            .get("model")
                            .or_else(|| value.pointer("/response/model"))
                            .and_then(Value::as_str)
                        {
                            model = upstream_model.to_string();
                        }
                        let chunk = chat_chunk(&id, &model, json!({"role":"assistant","content":""}), None, None);
                        yield Ok(chat_sse_data(&chunk));
                    }
                    "response.output_text.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            let chunk = chat_chunk(&id, &model, json!({"content": delta}), None, None);
                            yield Ok(chat_sse_data(&chunk));
                        }
                    }
                    "response.reasoning_summary_text.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            let chunk = chat_chunk(&id, &model, json!({"reasoning_content": delta}), None, None);
                            yield Ok(chat_sse_data(&chunk));
                        }
                    }
                    "response.output_item.added" => {
                        let item = value.get("item").unwrap_or(&value);
                        if item.get("type").and_then(Value::as_str) == Some("function_call") {
                            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("call_").to_string();
                            let index = *tool_slots.entry(call_id.clone()).or_insert_with(|| {
                                let index = next_tool_index;
                                next_tool_index += 1;
                                index
                            });
                            let chunk = chat_chunk(&id, &model, json!({
                                "tool_calls": [{
                                    "index": index,
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
                                        "arguments": ""
                                    }
                                }]
                            }), None, None);
                            yield Ok(chat_sse_data(&chunk));
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        let index = tool_slots.values().copied().max().unwrap_or(0);
                        let delta = value.get("delta").and_then(Value::as_str).unwrap_or("");
                        let chunk = chat_chunk(&id, &model, json!({
                            "tool_calls": [{
                                "index": index,
                                "function": { "arguments": delta }
                            }]
                        }), None, None);
                        yield Ok(chat_sse_data(&chunk));
                    }
                    "response.completed" => {
                        let response = value.get("response").unwrap_or(&value);
                        let usage = response.get("usage").cloned().unwrap_or(json!({}));
                        let input_tokens = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let output_tokens = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let finish = if response
                            .get("output")
                            .and_then(Value::as_array)
                            .is_some_and(|items| items.iter().any(|item| item.get("type").and_then(Value::as_str) == Some("function_call")))
                        {
                            "tool_calls"
                        } else {
                            "stop"
                        };
                        let chunk = chat_chunk(&id, &model, json!({}), Some(finish), Some(json!({
                            "prompt_tokens": input_tokens,
                            "completion_tokens": output_tokens,
                            "total_tokens": usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(input_tokens + output_tokens)
                        })));
                        yield Ok(chat_sse_data(&chunk));
                    }
                    _ => {}
                }
            }
        }
        yield Ok(Bytes::from("data: [DONE]\n\n"));
    }
}

/// Convert Chat Completions SSE to OpenAI Responses SSE.
pub fn chat_sse_to_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    request_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let response_id = gen_id("resp_");
        let message_id = gen_id("msg_");
        let mut model = request_model;
        let mut text_started = false;
        let mut text_buf = String::new();
        let mut completed = false;

        yield Ok(sse_json("response.created", &json!({
            "id": response_id.clone(),
            "object": "response",
            "status": "in_progress",
            "model": model.clone(),
            "output": []
        })));

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    continue;
                }
            };
            crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
            while let Some(block) = take_sse_block(&mut buffer) {
                for line in block.lines() {
                    let Some(data) = strip_sse_field(line, "data") else {
                        continue;
                    };
                    if data.trim() == "[DONE]" {
                        continue;
                    }
                    let value: Value = match serde_json::from_str(data) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    if let Some(upstream_model) = value.get("model").and_then(Value::as_str) {
                        model = upstream_model.to_string();
                    }
                    let choice = value
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|choices| choices.first())
                        .cloned()
                        .unwrap_or(json!({}));
                    if let Some(text) = choice.pointer("/delta/content").and_then(Value::as_str) {
                        if !text_started {
                            text_started = true;
                            yield Ok(sse_json("response.output_item.added", &json!({
                                "type": "message",
                                "id": message_id.clone(),
                                "status": "in_progress",
                                "role": "assistant",
                                "content": []
                            })));
                            yield Ok(sse_json("response.content_part.added", &json!({
                                "type": "output_text",
                                "text": ""
                            })));
                        }
                        text_buf.push_str(text);
                        yield Ok(sse_json("response.output_text.delta", &json!({
                            "type": "output_text",
                            "delta": text
                        })));
                    }
                    if choice.get("finish_reason").and_then(Value::as_str).is_some() && !completed {
                        completed = true;
                        if text_started {
                            yield Ok(sse_json("response.output_text.done", &json!({
                                "type": "output_text",
                                "text": text_buf.clone()
                            })));
                            yield Ok(sse_json("response.output_item.done", &json!({
                                "type": "message",
                                "id": message_id.clone(),
                                "status": "completed",
                                "role": "assistant",
                                "content": [{ "type": "output_text", "text": text_buf.clone() }]
                            })));
                        }
                        let usage = value.get("usage").cloned().unwrap_or(json!({}));
                        let input_tokens = usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let output_tokens = usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0);
                        yield Ok(sse_json("response.completed", &json!({
                            "id": response_id.clone(),
                            "object": "response",
                            "status": "completed",
                            "model": model.clone(),
                            "output": if text_started {
                                json!([{ "type": "message", "id": message_id.clone(), "status": "completed", "role": "assistant", "content": [{ "type": "output_text", "text": text_buf.clone() }] }])
                            } else {
                                json!([])
                            },
                            "usage": {
                                "input_tokens": input_tokens,
                                "output_tokens": output_tokens,
                                "total_tokens": usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(input_tokens + output_tokens)
                            }
                        })));
                    }
                }
            }
        }
    }
}

/// Convert Anthropic SSE to OpenAI Responses SSE through Chat chunks.
pub fn anthropic_sse_to_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    request_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    chat_sse_to_responses(
        anthropic_sse_to_chat(stream, request_model.clone()),
        request_model,
    )
}
