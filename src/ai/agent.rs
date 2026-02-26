use anyhow::Context;
use async_openai::types::{
    ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use futures::Stream;
use futures::StreamExt;
use rig::{
    client::CompletionClient,
    completion::{AssistantContent, Message as RigMessage},
    message::UserContent,
    providers::openai,
};

use crate::messages::models::Message;

#[derive(Clone)]
pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub temperature: f64,
    pub openai_api_key: String,
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

pub fn build_agent(config: AgentConfig) -> anyhow::Result<impl rig::completion::Prompt> {
    let client: openai::Client = openai::Client::new(&config.openai_api_key)
        .map_err(|e| anyhow::anyhow!("Failed to create OpenAI client: {e}"))?;

    Ok(client
        .agent(&config.model)
        .preamble(&config.system_prompt)
        .temperature(config.temperature)
        .build())
}

pub async fn stream_completion(
    config: &AgentConfig,
    content: &str,
    context_window: &[RigMessage],
) -> anyhow::Result<impl Stream<Item = anyhow::Result<String>>> {
    let _ = build_agent(config.clone())?;

    let openai_config =
        async_openai::config::OpenAIConfig::new().with_api_key(config.openai_api_key.clone());
    let client = async_openai::Client::with_config(openai_config);

    let mut messages: Vec<ChatCompletionRequestMessage> = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(config.system_prompt.clone())
            .build()
            .context("system message build")?
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
                        .context("user message build")?
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
                        .context("assistant message build")?
                        .into(),
                );
            }
        }
    }

    messages.push(
        ChatCompletionRequestUserMessageArgs::default()
            .content(content)
            .build()
            .context("final user message build")?
            .into(),
    );

    let request = CreateChatCompletionRequestArgs::default()
        .model(&config.model)
        .temperature(config.temperature as f32)
        .messages(messages)
        .stream(true)
        .build()
        .context("chat completion request build")?;

    let stream = client.chat().create_stream(request).await?;

    Ok(stream.map(|chunk| {
        chunk
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .map(|response| {
                response
                    .choices
                    .iter()
                    .filter_map(|choice| choice.delta.content.clone())
                    .collect::<Vec<_>>()
                    .join("")
            })
    }))
}
