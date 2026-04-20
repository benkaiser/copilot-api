use crate::anthropic::model_name::translate_model_name;
use crate::anthropic::types::*;
use crate::openai_types::*;

pub fn translate_to_openai(payload: &AnthropicMessagesPayload) -> ChatCompletionsPayload {
    let model = translate_model_name(&payload.model);

    // Build messages
    let mut messages: Vec<OpenAIMessage> = Vec::new();

    // System message
    if let Some(system) = &payload.system {
        let content = match system {
            SystemContent::Text(text) => text.clone(),
            SystemContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>()
                .join("\n\n"),
        };
        messages.push(OpenAIMessage {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Convert messages
    for msg in &payload.messages {
        match msg.role.as_str() {
            "user" => {
                translate_user_message(&msg.content, &mut messages);
            }
            "assistant" => {
                translate_assistant_message(&msg.content, &mut messages);
            }
            _ => {}
        }
    }

    // Convert tools
    let tools = payload.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: FunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: Some(t.input_schema.clone()),
                },
            })
            .collect()
    });

    // Convert tool_choice
    let tool_choice = payload.tool_choice.as_ref().map(|tc| match tc.choice_type.as_str() {
        "auto" => OpenAIToolChoice::String("auto".to_string()),
        "any" => OpenAIToolChoice::String("required".to_string()),
        "none" => OpenAIToolChoice::String("none".to_string()),
        "tool" => {
            if let Some(name) = &tc.name {
                OpenAIToolChoice::Object(ToolChoiceObject {
                    choice_type: "function".to_string(),
                    function: ToolChoiceFunction {
                        name: name.clone(),
                    },
                })
            } else {
                OpenAIToolChoice::String("auto".to_string())
            }
        }
        _ => OpenAIToolChoice::String("auto".to_string()),
    });

    // Reasoning effort from output_config or thinking
    let reasoning_effort = if let Some(output_config) = &payload.output_config {
        output_config.effort.clone()
    } else {
        payload.thinking.as_ref().and_then(|t| {
            if t.thinking_type != "enabled" {
                return None;
            }
            match t.budget_tokens {
                None => Some("medium".to_string()),
                Some(budget) if budget <= 4000 => Some("low".to_string()),
                Some(budget) if budget <= 16000 => Some("medium".to_string()),
                Some(_) => Some("high".to_string()),
            }
        })
    };

    ChatCompletionsPayload {
        model,
        messages,
        max_tokens: Some(payload.max_tokens),
        temperature: payload.temperature,
        top_p: payload.top_p,
        stream: payload.stream,
        stop: payload.stop_sequences.clone(),
        user: payload
            .metadata
            .as_ref()
            .and_then(|m| m.user_id.clone()),
        tools,
        tool_choice,
        reasoning_effort,
    }
}

fn translate_user_message(content: &AnthropicContent, messages: &mut Vec<OpenAIMessage>) {
    match content {
        AnthropicContent::Text(text) => {
            messages.push(OpenAIMessage {
                role: "user".to_string(),
                content: Some(MessageContent::Text(text.clone())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        AnthropicContent::Blocks(blocks) => {
            // Extract tool_result blocks as separate "tool" messages first
            let mut tool_messages: Vec<OpenAIMessage> = Vec::new();
            let mut remaining_blocks: Vec<&ContentBlock> = Vec::new();

            for block in blocks {
                match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        let text = content
                            .as_ref()
                            .map(|c| c.as_text())
                            .unwrap_or_default();
                        tool_messages.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(MessageContent::Text(text)),
                            name: None,
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                    _ => {
                        remaining_blocks.push(block);
                    }
                }
            }

            // Add tool messages first (before user message)
            messages.extend(tool_messages);

            // Build user message from remaining blocks
            if !remaining_blocks.is_empty() {
                let content = map_content_blocks(&remaining_blocks);
                messages.push(OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(content),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    }
}

fn translate_assistant_message(content: &AnthropicContent, messages: &mut Vec<OpenAIMessage>) {
    match content {
        AnthropicContent::Text(text) => {
            messages.push(OpenAIMessage {
                role: "assistant".to_string(),
                content: Some(MessageContent::Text(text.clone())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        AnthropicContent::Blocks(blocks) => {
            let has_tool_use = blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

            if has_tool_use {
                // Collect text/thinking content
                let text_parts: Vec<String> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.clone()),
                        ContentBlock::Thinking { thinking, .. } => Some(thinking.clone()),
                        _ => None,
                    })
                    .collect();

                let content = if text_parts.is_empty() {
                    None
                } else {
                    Some(MessageContent::Text(text_parts.join("\n\n")))
                };

                // Collect tool calls
                let tool_calls: Vec<ToolCall> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name: name.clone(),
                                arguments: serde_json::to_string(input).unwrap_or_default(),
                            },
                        }),
                        _ => None,
                    })
                    .collect();

                messages.push(OpenAIMessage {
                    role: "assistant".to_string(),
                    content,
                    name: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                });
            } else {
                let content = map_content_blocks(
                    &blocks.iter().collect::<Vec<_>>(),
                );
                messages.push(OpenAIMessage {
                    role: "assistant".to_string(),
                    content: Some(content),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    }
}

fn map_content_blocks(blocks: &[&ContentBlock]) -> MessageContent {
    let has_images = blocks
        .iter()
        .any(|b| matches!(b, ContentBlock::Image { .. }));

    if has_images {
        // Return as content parts array
        let parts: Vec<ContentPart> = blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(ContentPart::Text {
                    text: text.clone(),
                }),
                ContentBlock::Thinking { thinking, .. } => Some(ContentPart::Text {
                    text: thinking.clone(),
                }),
                ContentBlock::Image { source } => Some(ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: format!("data:{};base64,{}", source.media_type, source.data),
                    },
                }),
                _ => None,
            })
            .collect();
        MessageContent::Parts(parts)
    } else {
        // Join text blocks as a single string
        let texts: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                ContentBlock::Thinking { thinking, .. } => Some(thinking.clone()),
                _ => None,
            })
            .collect();
        MessageContent::Text(texts.join("\n\n"))
    }
}
