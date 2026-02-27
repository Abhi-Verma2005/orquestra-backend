use std::sync::Arc;

use anyhow::Context;
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use futures::StreamExt;
use rig::{
    completion::{AssistantContent, Message as RigMessage},
    message::UserContent,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{ai::tools::ToolRegistry, messages::models::Message};

#[derive(Clone)]
pub struct AgentConfig {
    pub model: String,
    pub temperature: f64,
    pub openai_api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    ToolCallsStarted {
        calls: Vec<ChatCompletionMessageToolCall>,
    },
    ToolExecuting {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: String,
        name: String,
        result: String,
    },
    Token(String),
    Done,
    Error(String),
}

pub struct AgentRunner {
    config: AgentConfig,
    tools: Arc<ToolRegistry>,
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
}

impl AgentRunner {
    pub fn new(config: AgentConfig, tools: Arc<ToolRegistry>) -> Self {
        let openai_config = async_openai::config::OpenAIConfig::new()
            .with_api_key(config.openai_api_key.clone());
        let client = async_openai::Client::with_config(openai_config);
        Self {
            config,
            tools,
            client,
        }
    }

    pub async fn run(
        &self,
        messages: Vec<ChatCompletionRequestMessage>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> anyhow::Result<String> {
        let mut conversation = messages;
        let tools = self.tools.to_openai_tools();
        let max_iterations = 10;

        for iteration in 0..max_iterations {
            tracing::debug!(iteration = iteration + 1, "Agent loop iteration");

            let request = CreateChatCompletionRequestArgs::default()
                .model(&self.config.model)
                .temperature(self.config.temperature as f32)
                .messages(conversation.clone())
                .tools(tools.clone())
                .build()
                .context("build loop request")?;

            let response = self.client.chat().create(request).await?;
            let choice = response
                .choices
                .first()
                .ok_or_else(|| anyhow::anyhow!("No response choices"))?;

            if let Some(tool_calls) = &choice.message.tool_calls {
                if !tool_calls.is_empty() {
                    let _ = event_tx.send(AgentEvent::ToolCallsStarted {
                        calls: tool_calls.clone(),
                    });

                    let mut assistant_builder = ChatCompletionRequestAssistantMessageArgs::default();
                    if let Some(content) = choice.message.content.clone() {
                        assistant_builder.content(content);
                    }
                    let assistant_message = assistant_builder
                        .tool_calls(tool_calls.clone())
                        .build()
                        .context("build assistant tool-call message")?;
                    conversation.push(assistant_message.into());

                    for tool_call in tool_calls {
                        let tool_name = &tool_call.function.name;
                        let arguments: Value = serde_json::from_str(&tool_call.function.arguments)
                            .context("parse tool call arguments")?;

                        let _ = event_tx.send(AgentEvent::ToolExecuting {
                            id: tool_call.id.clone(),
                            name: tool_name.clone(),
                            arguments: arguments.clone(),
                        });

                        let result = match self.tools.get(tool_name) {
                            Some(tool) => tool
                                .execute(arguments)
                                .await
                                .unwrap_or_else(|e| format!("Error: {e}")),
                            None => format!("Unknown tool: {tool_name}"),
                        };

                        let _ = event_tx.send(AgentEvent::ToolResult {
                            id: tool_call.id.clone(),
                            name: tool_name.clone(),
                            result: result.clone(),
                        });

                        conversation.push(
                            ChatCompletionRequestToolMessageArgs::default()
                                .tool_call_id(&tool_call.id)
                                .content(result)
                                .build()
                                .context("build tool result message")?
                                .into(),
                        );
                    }

                    continue;
                }
            }

            return self.stream_final_response(&conversation, &event_tx).await;
        }

        Err(anyhow::anyhow!("Max iterations reached"))
    }

    async fn stream_final_response(
        &self,
        messages: &[ChatCompletionRequestMessage],
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> anyhow::Result<String> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .temperature(self.config.temperature as f32)
            .messages(messages.to_vec())
            .stream(true)
            .build()
            .context("build final stream request")?;

        let mut stream = self.client.chat().create_stream(request).await?;
        let mut full_response = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            for choice in &chunk.choices {
                if let Some(content) = &choice.delta.content {
                    full_response.push_str(content);
                    let _ = event_tx.send(AgentEvent::Token(content.clone()));
                }
            }
        }

        let _ = event_tx.send(AgentEvent::Done);
        Ok(full_response)
    }
}

pub fn build_context_window(messages: &[Message]) -> Vec<RigMessage> {
    let window: Vec<&Message> = messages
        .iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    window
        .iter()
        .map(|msg| {
            if msg.role == "user" {
                RigMessage::user(msg.content.clone())
            } else {
                RigMessage::assistant(msg.content.clone())
            }
        })
        .collect()
}

pub fn build_system_prompt(
    base_prompt: Option<&str>,
    user_first_name: &str,
    user_last_name: &str,
    user_email: &str,
) -> String {
    let user_context = format!(
        "The user's full name is {user_first_name} {user_last_name}. Their email is {user_email}. Address them by their first name ({user_first_name}) when greeting."
    );

    match base_prompt {
        Some(prompt) if !prompt.trim().is_empty() => format!("{prompt}\n\n{user_context}"),
        _ => format!(
            "You are Halo, a helpful and intelligent AI assistant. You are thoughtful, concise, and accurate.\n\n{user_context}"
        ),
    }
}

pub fn build_chat_messages(
    context_window: &[RigMessage],
    user_message: &str,
    system_prompt: &str,
) -> anyhow::Result<Vec<ChatCompletionRequestMessage>> {
    let mut messages: Vec<ChatCompletionRequestMessage> = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt)
            .build()
            .context("build system prompt")?
            .into(),
    ];

    for message in context_window {
        match message {
            RigMessage::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        UserContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                messages.push(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(text)
                        .build()
                        .context("build user history message")?
                        .into(),
                );
            }
            RigMessage::Assistant { content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        AssistantContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                messages.push(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .content(text)
                        .build()
                        .context("build assistant history message")?
                        .into(),
                );
            }
        }
    }

    messages.push(
        ChatCompletionRequestUserMessageArgs::default()
            .content(user_message)
            .build()
            .context("build final user message")?
            .into(),
    );

    Ok(messages)
}
