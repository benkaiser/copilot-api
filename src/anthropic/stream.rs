use std::collections::HashMap;

use crate::anthropic::translate_response::translate_stop_reason;
use crate::anthropic::types::*;
use crate::openai_types::ChatCompletionChunk;

#[derive(Debug)]
#[allow(dead_code)]
struct ToolCallInfo {
    id: String,
    name: String,
    anthropic_block_index: usize,
}

#[derive(Debug)]
pub struct StreamState {
    message_start_sent: bool,
    content_block_index: usize,
    content_block_open: bool,
    tool_calls: HashMap<u64, ToolCallInfo>,
    model: String,
    original_model: String,
}

impl StreamState {
    pub fn new(original_model: &str) -> Self {
        Self {
            message_start_sent: false,
            content_block_index: 0,
            content_block_open: false,
            tool_calls: HashMap::new(),
            model: String::new(),
            original_model: original_model.to_string(),
        }
    }
}

pub fn translate_chunk_to_anthropic_events(
    chunk: &ChatCompletionChunk,
    state: &mut StreamState,
) -> Vec<AnthropicStreamEvent> {
    let mut events: Vec<AnthropicStreamEvent> = Vec::new();

    if chunk.choices.is_empty() {
        return events;
    }

    state.model = chunk.model.clone();

    // Build usage info from chunk
    let cached_tokens = chunk
        .usage
        .as_ref()
        .and_then(|u| u.prompt_tokens_details.as_ref())
        .map(|d| d.cached_tokens)
        .unwrap_or(0);
    let prompt_tokens = chunk
        .usage
        .as_ref()
        .map(|u| u.prompt_tokens)
        .unwrap_or(0);
    let input_tokens = prompt_tokens.saturating_sub(cached_tokens);
    let cache_read = if cached_tokens > 0 {
        Some(cached_tokens)
    } else {
        None
    };

    // Emit message_start if not sent yet
    if !state.message_start_sent {
        events.push(AnthropicStreamEvent::MessageStart {
            message: AnthropicResponse {
                id: chunk.id.clone(),
                response_type: "message".to_string(),
                role: "assistant".to_string(),
                model: state.original_model.clone(),
                content: Vec::new(),
                stop_reason: None,
                stop_sequence: None,
                usage: AnthropicUsage {
                    input_tokens,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: cache_read,
                    service_tier: None,
                },
            },
        });
        state.message_start_sent = true;
    }

    for choice in &chunk.choices {
        let delta = &choice.delta;

        // Handle text content
        if let Some(text) = &delta.content {
            // Close any open tool block first
            if state.content_block_open
                && state
                    .tool_calls
                    .values()
                    .any(|tc| tc.anthropic_block_index == state.content_block_index)
            {
                events.push(AnthropicStreamEvent::ContentBlockStop {
                    index: state.content_block_index,
                });
                state.content_block_index += 1;
                state.content_block_open = false;
            }

            if !state.content_block_open {
                events.push(AnthropicStreamEvent::ContentBlockStart {
                    index: state.content_block_index,
                    content_block: StreamContentBlock::Text {
                        text: String::new(),
                    },
                });
                state.content_block_open = true;
            }

            events.push(AnthropicStreamEvent::ContentBlockDelta {
                index: state.content_block_index,
                delta: StreamDelta::TextDelta {
                    text: text.clone(),
                },
            });
        }

        // Handle tool calls
        if let Some(tool_calls) = &delta.tool_calls {
            for tc in tool_calls {
                let openai_index = tc.index;

                // New tool call (has id and function name)
                if let (Some(id), Some(func)) = (&tc.id, &tc.function) {
                    if let Some(name) = &func.name {
                        // Close any open block
                        if state.content_block_open {
                            events.push(AnthropicStreamEvent::ContentBlockStop {
                                index: state.content_block_index,
                            });
                            state.content_block_index += 1;
                            state.content_block_open = false;
                        }

                        let anthropic_index = state.content_block_index;
                        state.tool_calls.insert(
                            openai_index,
                            ToolCallInfo {
                                id: id.clone(),
                                name: name.clone(),
                                anthropic_block_index: anthropic_index,
                            },
                        );

                        events.push(AnthropicStreamEvent::ContentBlockStart {
                            index: anthropic_index,
                            content_block: StreamContentBlock::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: serde_json::json!({}),
                            },
                        });
                        state.content_block_open = true;
                    }
                }

                // Tool call arguments delta
                if let Some(func) = &tc.function {
                    if let Some(arguments) = &func.arguments {
                        if !arguments.is_empty() {
                            let index = state
                                .tool_calls
                                .get(&openai_index)
                                .map(|info| info.anthropic_block_index)
                                .unwrap_or(state.content_block_index);

                            events.push(AnthropicStreamEvent::ContentBlockDelta {
                                index,
                                delta: StreamDelta::InputJsonDelta {
                                    partial_json: arguments.clone(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(finish_reason) = &choice.finish_reason {
            // Close any open block
            if state.content_block_open {
                events.push(AnthropicStreamEvent::ContentBlockStop {
                    index: state.content_block_index,
                });
                state.content_block_open = false;
            }

            let stop_reason = translate_stop_reason(Some(finish_reason.as_str()));
            let completion_tokens = chunk
                .usage
                .as_ref()
                .map(|u| u.completion_tokens)
                .unwrap_or(0);

            events.push(AnthropicStreamEvent::MessageDelta {
                delta: MessageDeltaContent {
                    stop_reason,
                    stop_sequence: None,
                },
                usage: MessageDeltaUsage {
                    input_tokens,
                    output_tokens: completion_tokens,
                    cache_read_input_tokens: cache_read,
                },
            });

            events.push(AnthropicStreamEvent::MessageStop {});
        }
    }

    events
}

pub fn format_sse_event(event: &AnthropicStreamEvent) -> String {
    let event_type = match event {
        AnthropicStreamEvent::MessageStart { .. } => "message_start",
        AnthropicStreamEvent::ContentBlockStart { .. } => "content_block_start",
        AnthropicStreamEvent::ContentBlockDelta { .. } => "content_block_delta",
        AnthropicStreamEvent::ContentBlockStop { .. } => "content_block_stop",
        AnthropicStreamEvent::MessageDelta { .. } => "message_delta",
        AnthropicStreamEvent::MessageStop { .. } => "message_stop",
        AnthropicStreamEvent::Ping { .. } => "ping",
        AnthropicStreamEvent::Error { .. } => "error",
    };

    let data = serde_json::to_string(event).unwrap_or_default();
    format!("event: {}\ndata: {}\n\n", event_type, data)
}
