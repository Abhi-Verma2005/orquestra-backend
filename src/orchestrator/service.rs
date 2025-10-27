// src/orchestrator/service.rs
use crate::{
    config::Config,
    llm_provider::gemini::{call_gemini_api, Available_Functions, GeminiModel},
    models::{ChatMessage, Role, messages_to_prompt},
};

pub struct OrchestratorService {
    config: Config,
}

impl OrchestratorService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.run_demo().await;
        Ok(())
    }

    async fn run_demo(&self) {
        // Move your AI orchestration demo logic here
        // ... (all the existing demo code from main.rs)
    }
}