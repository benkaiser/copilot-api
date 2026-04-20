use crate::anthropic::types::*;
use crate::openai_types::*;

pub fn translate_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| match r {
        "stop" => "end_turn".to_string(),
        "length" => "max_tokens".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "content_filter" => "end_turn".to_string(),
        other => other.to_string(),
    })
}

pub fn translate_to_anthropic(
    response: &ChatCompletionResponse,
    original_model: &str,
) -> AnthropicResponse {
    let mut content: Vec<ResponseContentBlock> = Vec::new();

    // Merge all choices
    for choice in &response.choices {
        // Text content
        if let Some(msg_content) = &choice.message.content {
            match msg_content {
                MessageContent::Text(text) => {
                    if !text.is_empty() {
                        content.push(ResponseContentBlock::Text {
                            text: text.clone(),
                        });
                    }
                }
                MessageContent::Parts(parts) => {
                    for part in parts {
                        if let ContentPart::Text { text } = part {
                            if !text.is_empty() {
                                content.push(ResponseContentBlock::Text {
                                    text: text.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Tool calls
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                content.push(ResponseContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                });
            }
        }
    }

    // Get stop reason from last choice
    let stop_reason = response
        .choices
        .last()
        .and_then(|c| translate_stop_reason(c.finish_reason.as_deref()));

    // Build usage
    let usage = response.usage.as_ref();
    let cached_tokens = usage
        .and_then(|u| u.prompt_tokens_details.as_ref())
        .map(|d| d.cached_tokens)
        .unwrap_or(0);

    let prompt_tokens = usage.map(|u| u.prompt_tokens).unwrap_or(0);
    let input_tokens = prompt_tokens.saturating_sub(cached_tokens);

    let cache_read = if cached_tokens > 0 {
        Some(cached_tokens)
    } else {
        None
    };

    AnthropicResponse {
        id: response.id.clone(),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        model: original_model.to_string(),
        content,
        stop_reason,
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens,
            output_tokens: usage.map(|u| u.completion_tokens).unwrap_or(0),
            cache_creation_input_tokens: None,
            cache_read_input_tokens: cache_read,
            service_tier: None,
        },
    }
}
