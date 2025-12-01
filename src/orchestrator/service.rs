// src/orchestrator/service.rs
use crate::{
    config::Config,
    llm_provider::openai::{call_openai_api, OpenAIModel},
    models::ChatMessage,
};

pub struct OrchestratorService {
    config: Config,
}

impl OrchestratorService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Process user input and return AI response
    pub async fn process_message(&self, user_input: &str) -> Result<String, String> {
        println!("🤖 [Orchestrator] Processing user input: {}", user_input);
        
        // Simple prompt - just send user input to AI
        println!("📡 [Orchestrator] Calling OpenAI API...");
        let response = call_openai_api(
            user_input,
            OpenAIModel::Gpt35Turbo,
            &self.config.openai_api_key,
        ).await.map_err(|e| {
            println!("❌ [Orchestrator] OpenAI API error: {:?}", e);
            format!("OpenAI API error: {:?}", e)
        })?;

        println!("✅ [Orchestrator] Received response from OpenAI API");
        println!("📝 [Orchestrator] Response text ({} chars): {}", response.len(), response.chars().take(100).collect::<String>());
        
        Ok(response)
    }

    /// Process chat message and return formatted response
    pub async fn process_chat_message(&self, chat_message: &ChatMessage) -> Result<ChatMessage, String> {
        println!("💬 [Orchestrator] Processing chat message - Role: {:?}, Content: {}", chat_message.role, chat_message.content);
        let response_text = self.process_message(&chat_message.content).await?;
        
        let response = ChatMessage {
            role: crate::models::Role::Assistant,
            content: response_text.clone(),
            name: None,
        };
        
        println!("✅ [Orchestrator] Created response message: {}", response.content.chars().take(100).collect::<String>());
        Ok(response)
    }
}