use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use code_otel::otel_event_manager::OtelEventManager;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use reqwest::StatusCode;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

use crate::auth::AuthManager;
use crate::ModelProviderInfo;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::client_common::replace_image_payloads_for_model;
use crate::client_common::rewrite_image_generation_calls_for_input;
use crate::debug_logger::DebugLogger;
use crate::error::CodexErr;
use crate::error::Result;
use crate::error::RetryLimitReachedError;
use crate::error::UnexpectedResponseError;
use crate::model_family::ModelFamily;
use crate::openai_tools::create_tools_json_for_anthropic;
use crate::util::backoff;
use code_protocol::models::ContentItem;
use code_protocol::models::ReasoningItemContent;
use code_protocol::models::ResponseItem;
use crate::protocol::TokenUsage;

pub(crate) async fn stream_anthropic_messages(
    prompt: &Prompt,
    model_family: &ModelFamily,
    model_slug: &str,
    client: &reqwest::Client,
    provider: &ModelProviderInfo,
    debug_logger: &Arc<Mutex<DebugLogger>>,
    auth_manager: Option<Arc<AuthManager>>,
    otel_event_manager: Option<OtelEventManager>,
    log_tag: Option<&str>,
) -> Result<ResponseStream> {
    let payload = build_anthropic_request(prompt, model_family, model_slug, provider)?;
    debug!("Anthropic request payload: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());

    let endpoint = provider.get_full_url(&None);
    let mut attempt = 0;
    let max_retries = provider.request_max_retries();
    let mut request_id = String::new();

    loop {
        attempt += 1;

        let base_auth = auth_manager.as_ref().and_then(|m| m.auth());
        let auth = provider.effective_auth(&base_auth).await?;
        let mut req_builder = provider.create_request_builder_with_auth(client, &auth).await?;

        req_builder = req_builder
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&payload);

        if request_id.is_empty() {
            let endpoint_for_log = provider.get_full_url(&auth);
            let header_snapshot = req_builder
                .try_clone()
                .and_then(|builder| builder.build().ok())
                .map(|req| crate::chat_completions::header_map_to_json(req.headers()));

            if let Ok(logger) = debug_logger.lock() {
                request_id = logger
                    .start_request_log(
                        &endpoint_for_log,
                        &payload,
                        header_snapshot.as_ref(),
                        log_tag,
                    )
                    .unwrap_or_default();
            }
        }

        let res = req_builder.send().await;

        match res {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(logger) = debug_logger.lock() {
                    let _ = logger.append_response_event(
                        &request_id,
                        "stream_initiated",
                        &serde_json::json!({
                            "status": "success",
                            "status_code": resp.status().as_u16()
                        }),
                    );
                }
                let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);
                let stream = resp.bytes_stream().map_err(CodexErr::Reqwest);
                let debug_logger_clone = Arc::clone(&debug_logger);
                let request_id_clone = request_id.clone();
                tokio::spawn(process_anthropic_sse(
                    stream,
                    tx_event,
                    provider.stream_idle_timeout(),
                    debug_logger_clone,
                    request_id_clone,
                    otel_event_manager.clone(),
                ));
                return Ok(ResponseStream { rx_event });
            }
            Ok(res) => {
                let status = res.status();
                if status == StatusCode::UNAUTHORIZED && provider.has_command_auth() {
                    provider.invalidate_cached_auth_token();
                    if attempt > max_retries {
                        return Err(CodexErr::RetryLimit(RetryLimitReachedError {
                            status,
                            request_id: None,
                            retryable: true,
                        }));
                    }
                    let delay = backoff(attempt);
                    tokio::time::sleep(delay).await;
                    continue;
                }
                let body_text = res.text().await.unwrap_or_default();
                let error_detail = try_parse_anthropic_error(&body_text);
                let msg = format!(
                    "Anthropic API error (HTTP {}): {}",
                    status,
                    error_detail.unwrap_or(body_text.clone())
                );
                if status.is_server_error() {
                    return Err(CodexErr::ServerError(msg));
                }
                if status == StatusCode::TOO_MANY_REQUESTS {
                    if attempt > max_retries {
                        return Err(CodexErr::RetryLimit(RetryLimitReachedError {
                            status,
                            request_id: None,
                            retryable: true,
                        }));
                    }
                    let delay = backoff(attempt);
                    tokio::time::sleep(delay).await;
                    continue;
                }
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    return Err(CodexErr::AuthRefreshPermanent(msg));
                }
                return Err(CodexErr::UnexpectedStatus(UnexpectedResponseError {
                    status,
                    body: body_text,
                    request_id: None,
                }));
            }
            Err(e) => {
                if attempt > max_retries {
                    return Err(CodexErr::RetryLimit(RetryLimitReachedError {
                        status: StatusCode::BAD_GATEWAY,
                        request_id: None,
                        retryable: true,
                    }));
                }
                let delay = backoff(attempt);
                tokio::time::sleep(delay).await;
                continue;
            }
        }
    }
}

fn try_parse_anthropic_error(body: &str) -> Option<String> {
    if let Ok(val) = serde_json::from_str::<Value>(body) {
        if let Some(error_obj) = val.get("error") {
            let error_type = error_obj.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
            let message = error_obj.get("message").and_then(|v| v.as_str()).unwrap_or("");
            return Some(format!("[{}] {}", error_type, message));
        }
    }
    None
}

fn build_anthropic_request(
    prompt: &Prompt,
    model_family: &ModelFamily,
    model_slug: &str,
    provider: &ModelProviderInfo,
) -> Result<Value> {
    let mut input = prompt.get_formatted_input();
    rewrite_image_generation_calls_for_input(&mut input);
    replace_image_payloads_for_model(&mut input, model_slug);

    let full_instructions = prompt.get_full_instructions(model_family);

    let (system_text, messages) = convert_input_to_anthropic_messages(&input, &full_instructions);

    let tools_json = create_tools_json_for_anthropic(&prompt.tools)?;

    let max_tokens = model_family.max_output_tokens.unwrap_or(4096);

    let mut payload = json!({
        "model": model_slug,
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": true,
    });

    if !system_text.is_empty() {
        payload.as_object_mut().unwrap().insert(
            "system".to_string(),
            Value::String(system_text),
        );
    }

    if !tools_json.is_empty() {
        payload.as_object_mut().unwrap().insert(
            "tools".to_string(),
            Value::Array(tools_json),
        );
    }

    Ok(payload)
}

fn convert_input_to_anthropic_messages(
    input: &[ResponseItem],
    full_instructions: &str,
) -> (String, Vec<Value>) {
    let mut system_text = full_instructions.to_string();
    let mut messages: Vec<Value> = Vec::new();

    struct PendingAssistant {
        content_blocks: Vec<Value>,
    }

    let mut pending_assistant: Option<PendingAssistant> = None;

    let mut flush_assistant = |messages: &mut Vec<Value>, pending: &mut Option<PendingAssistant>| {
        if let Some(assistant) = pending.take() {
            if !assistant.content_blocks.is_empty() {
                messages.push(json!({"role": "assistant", "content": assistant.content_blocks}));
            }
        }
    };

    for item in input {
        match item {
            ResponseItem::Message { role, content, .. }
                if role == "developer" || role == "system" =>
            {
                for c in content {
                    if let ContentItem::InputText { text } = c {
                        system_text.push('\n');
                        system_text.push_str(text);
                    }
                }
            }
            ResponseItem::Message { role, content, .. } if role == "user" => {
                flush_assistant(&mut messages, &mut pending_assistant);
                let blocks = convert_content_to_anthropic_blocks(content);
                messages.push(json!({"role": "user", "content": blocks}));
            }
            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                flush_assistant(&mut messages, &mut pending_assistant);
                let mut blocks = Vec::new();
                for c in content {
                    match c {
                        ContentItem::OutputText { text } => {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                        _ => {}
                    }
                }
                pending_assistant = Some(PendingAssistant { content_blocks: blocks });
            }
            ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                let args_value: Value =
                    serde_json::from_str(arguments).unwrap_or(Value::Object(Default::default()));
                let block = json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": args_value,
                });
                if let Some(ref mut pending) = pending_assistant {
                    pending.content_blocks.push(block);
                }
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                flush_assistant(&mut messages, &mut pending_assistant);
                let output_text = output.to_string();
                messages.push(json!({
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": call_id, "content": output_text}]
                }));
            }
            _ => {}
        }
    }
    flush_assistant(&mut messages, &mut pending_assistant);

    (system_text, messages)
}

fn convert_content_to_anthropic_blocks(content: &[ContentItem]) -> Vec<Value> {
    let mut blocks = Vec::new();
    for c in content {
        match c {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                blocks.push(json!({"type": "text", "text": text}));
            }
            ContentItem::InputImage { image_url } => {
                if let Some(block) = parse_image_data_uri(image_url) {
                    blocks.push(block);
                }
            }
            _ => {}
        }
    }
    blocks
}

fn parse_image_data_uri(image_url: &str) -> Option<Value> {
    let rest = image_url.strip_prefix("data:")?;
    let (media_type, after) = rest.split_once(';')?;
    if !after.starts_with("base64,") {
        return None;
    }
    let data = after.trim_start_matches("base64,");
    Some(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": data,
        }
    }))
}

struct AnthropicSseState {
    assistant_text: String,
    reasoning_text: String,
    current_item_id: Option<String>,
    current_response_id: Option<String>,
    current_response_model: Option<String>,
    block_type: Option<String>,
    block_index: Option<u32>,
    tool_use_id: Option<String>,
    tool_use_name: Option<String>,
    tool_use_input: String,
    token_usage: Option<TokenUsage>,
}

async fn process_anthropic_sse<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent>>,
    idle_timeout: Duration,
    debug_logger: Arc<Mutex<DebugLogger>>,
    request_id: String,
    otel_event_manager: Option<OtelEventManager>,
) where
    S: Stream<Item = Result<Bytes>> + Unpin,
{
    let mut stream = stream.eventsource();

    let mut state = AnthropicSseState {
        assistant_text: String::new(),
        reasoning_text: String::new(),
        current_item_id: None,
        current_response_id: None,
        current_response_model: None,
        block_type: None,
        block_index: None,
        tool_use_id: None,
        tool_use_name: None,
        tool_use_input: String::new(),
        token_usage: None,
    };

    async fn flush_and_complete(
        tx_event: &mpsc::Sender<Result<ResponseEvent>>,
        state: &mut AnthropicSseState,
        debug_logger: &Arc<Mutex<DebugLogger>>,
        request_id: &str,
    ) {
        if !state.assistant_text.is_empty() {
            let item = ResponseItem::Message {
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: std::mem::take(&mut state.assistant_text),
                }],
                id: state.current_item_id.clone(),
                end_turn: None,
                phase: None,
            };
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputItemDone {
                    item,
                    sequence_number: None,
                    output_index: None,
                }))
                .await;
        }

        if !state.reasoning_text.is_empty() {
            let item = ResponseItem::Reasoning {
                id: state.current_item_id.clone().unwrap_or_default(),
                summary: Vec::new(),
                content: Some(vec![ReasoningItemContent::ReasoningText {
                    text: std::mem::take(&mut state.reasoning_text),
                }]),
                encrypted_content: None,
            };
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputItemDone {
                    item,
                    sequence_number: None,
                    output_index: None,
                }))
                .await;
        }

        let _ = tx_event
            .send(Ok(ResponseEvent::Completed {
                response_id: state.current_response_id.clone().unwrap_or_default(),
                token_usage: state.token_usage.take(),
            }))
            .await;
        if let Ok(logger) = debug_logger.lock() {
            let _ = logger.end_request_log(request_id);
        }
    }

    loop {
        let next_event = if let Some(manager) = otel_event_manager.as_ref() {
            manager
                .log_sse_event(|| timeout(idle_timeout, stream.next()))
                .await
        } else {
            timeout(idle_timeout, stream.next()).await
        };

        let sse = match next_event {
            Ok(Some(Ok(ev))) => ev,
            Ok(Some(Err(e))) => {
                let _ = tx_event
                    .send(Err(CodexErr::Stream(
                        format!("[transport] {e}"),
                        None,
                        Some(request_id.clone()),
                    )))
                    .await;
                return;
            }
            Ok(None) => {
                tracing::debug!("anthropic SSE stream closed without message_stop");
                if let Ok(logger) = debug_logger.lock() {
                    let _ = logger.append_response_event(
                        &request_id,
                        "stream_closed_without_stop",
                        &serde_json::json!({
                            "assistant_len": state.assistant_text.len(),
                            "reasoning_len": state.reasoning_text.len(),
                        }),
                    );
                }
                flush_and_complete(&tx_event, &mut state, &debug_logger, &request_id).await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(CodexErr::Stream(
                        "[idle] timeout waiting for SSE".into(),
                        None,
                        Some(request_id.clone()),
                    )))
                    .await;
                return;
            }
        };

        let data = sse.data.trim();
        if data.is_empty() {
            continue;
        }

        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                let mut excerpt = sse.data.clone();
                const MAX: usize = 600;
                if excerpt.len() > MAX {
                    excerpt.truncate(MAX);
                }
                tracing::debug!("anthropic SSE parse error: {} | data: {}", e, excerpt);
                if let Ok(logger) = debug_logger.lock() {
                    let _ = logger.append_response_event(
                        &request_id,
                        "sse_parse_error",
                        &serde_json::json!({
                            "error": e.to_string(),
                            "data_excerpt": excerpt,
                        }),
                    );
                }
                continue;
            }
        };
        trace!("anthropic received SSE chunk: {chunk:?}");

        if let Ok(logger) = debug_logger.lock() {
            let _ = logger.append_response_event(&request_id, "sse_event", &chunk);
        }

        let event_type = chunk.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "message_start" => {
                state.current_response_id = chunk
                    .get("message")
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                state.current_response_model = chunk
                    .get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let _ = tx_event
                    .send(Ok(ResponseEvent::Created {
                        response_id: state.current_response_id.clone(),
                        response_model: state.current_response_model.clone(),
                    }))
                    .await;
            }
            "content_block_start" => {
                state.block_index = chunk.get("index").and_then(|v| v.as_u64()).map(|v| v as u32);
                state.block_type = chunk
                    .get("content_block")
                    .and_then(|cb| cb.get("type"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                match state.block_type.as_deref() {
                    Some("tool_use") => {
                        state.tool_use_id = chunk
                            .get("content_block")
                            .and_then(|cb| cb.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        state.tool_use_name = chunk
                            .get("content_block")
                            .and_then(|cb| cb.get("name"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        state.tool_use_input.clear();
                    }
                    Some("thinking") => {
                        state.reasoning_text.clear();
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let delta = chunk.get("delta");
                let delta_type = delta
                    .and_then(|d| d.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta.and_then(|d| d.get("text")).and_then(|v| v.as_str()) {
                            state.assistant_text.push_str(text);
                            let _ = tx_event
                                .send(Ok(ResponseEvent::OutputTextDelta {
                                    delta: text.to_string(),
                                    item_id: state.current_item_id.clone(),
                                    sequence_number: None,
                                    output_index: state.block_index,
                                }))
                                .await;
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta.and_then(|d| d.get("thinking")).and_then(|v| v.as_str()) {
                            state.reasoning_text.push_str(text);
                            let _ = tx_event
                                .send(Ok(ResponseEvent::ReasoningContentDelta {
                                    delta: text.to_string(),
                                    item_id: state.current_item_id.clone(),
                                    sequence_number: None,
                                    output_index: state.block_index,
                                    content_index: state.block_index,
                                }))
                                .await;
                        }
                    }
                    "input_json_delta" => {
                        if let Some(partial) = delta
                            .and_then(|d| d.get("partial_json"))
                            .and_then(|v| v.as_str())
                        {
                            state.tool_use_input.push_str(partial);
                        }
                    }
                    _ => {
                        tracing::debug!("unknown anthropic delta type: {delta_type}");
                    }
                }
            }
            "content_block_stop" => {
                match state.block_type.as_deref() {
                    Some("tool_use") => {
                        let args_value: Value = serde_json::from_str(&state.tool_use_input)
                            .unwrap_or(Value::Object(Default::default()));
                        let arguments_str = serde_json::to_string(&args_value)
                            .unwrap_or_else(|_| state.tool_use_input.clone());
                        let item = ResponseItem::FunctionCall {
                            id: state.tool_use_id.clone(),
                            name: state.tool_use_name.clone().unwrap_or_default(),
                            namespace: None,
                            arguments: arguments_str,
                            call_id: state.tool_use_id.clone().unwrap_or_default(),
                        };
                        let _ = tx_event
                            .send(Ok(ResponseEvent::OutputItemDone {
                                item,
                                sequence_number: None,
                                output_index: state.block_index,
                            }))
                            .await;
                        state.tool_use_id = None;
                        state.tool_use_name = None;
                        state.tool_use_input.clear();
                    }
                    Some("thinking") => {
                        // thinking text already emitted via ReasoningContentDelta events
                    }
                    Some("text") => {
                        // text already emitted via OutputTextDelta events
                    }
                    _ => {}
                }
                state.block_type = None;
                state.block_index = None;
            }
            "message_delta" => {
                if let Some(usage) = chunk.get("usage") {
                    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    state.token_usage = Some(TokenUsage {
                        input_tokens,
                        cached_input_tokens: 0,
                        output_tokens,
                        reasoning_output_tokens: 0,
                        total_tokens: input_tokens + output_tokens,
                    });
                }
            }
            "message_stop" => {
                flush_and_complete(&tx_event, &mut state, &debug_logger, &request_id).await;
                return;
            }
            "ping" => {}
            _ => {
                tracing::debug!("unhandled anthropic event type: {event_type}");
            }
        }
    }
}
