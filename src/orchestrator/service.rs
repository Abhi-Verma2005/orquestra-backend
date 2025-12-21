use std::sync::Arc;

use sqlx::PgPool;

// src/orchestrator/service.rs
use crate::{
    config::Config, db::service::ChatDbService, llm_provider::gemini::{GeminiModel, call_gemini_text}, models::{ChatMessage, Role}, services::{
        cache::CacheService, ethereum_client::EthereumClient, metadata_service::MetadataService,
        price_service::PriceService, solana_client::SolanaClient,
    }, utils::gen_id::generate_id
};

pub struct OrchestratorService {
    config: Config,
    solana_client: SolanaClient,
    ethereum_client: EthereumClient,
    chat_db: ChatDbService
}

impl OrchestratorService {
    pub fn new(config: Config, chat_db: ChatDbService) -> Self {
        // Initialize blockchain services
        let cache = CacheService::new();
        let price_service = PriceService::new(cache.clone());
        let metadata_service = MetadataService::new(cache.clone());
        let solana_client = SolanaClient::new(

            config.solana_rpc_url.clone(),
            price_service.clone(),
            metadata_service.clone(),
            config.clone(),
        );

        let ethereum_client = EthereumClient::new(
            config.ethereum_rpc_url.clone(),
            price_service.clone(),
            metadata_service.clone(),
            config.clone(),
        );

        Self {
            config,
            solana_client,
            ethereum_client,
            chat_db
        }
    }

    /// Get blockchain clients for tool execution
    pub fn get_solana_client(&self) -> &SolanaClient {
        &self.solana_client
    }

    pub fn get_ethereum_client(&self) -> &EthereumClient {
        &self.ethereum_client
    }

    /// Detect if wallet tool call is needed based on user input
    pub async fn detect_wallet_intent(&self, user_input: &str) -> Result<bool, String> {
        let intent_prompt = format!(
            r#"Analyze the following user query and determine if it requires fetching wallet balance or portfolio data from a blockchain (Solana or Ethereum).

User query: "{}"

Respond with ONLY one word: "yes" if wallet data is needed (e.g., "show my balance", "what's in my wallet", "check my portfolio"), or "no" if it's a general question that doesn't require blockchain data.

Response:"#,
            user_input
        );

        let response = call_gemini_text(
            &intent_prompt,
            GeminiModel::GeminiLite,
            &self.config.gemini_api_key,
        )
        .await
        .map_err(|e| format!("Intent detection error: {:?}", e))?;

        let intent_needed = response.trim().to_lowercase().contains("yes");

        Ok(intent_needed)
    }

    /// Generate acknowledgment message after tool execution
    pub async fn generate_acknowledgment(
        &self,
        user_query: &str,
        tool_results: &str,
    ) -> Result<String, String> {
        let ack_prompt = format!(
            r#"The user asked: "{}"

I fetched their wallet data. Here's what I found:
{}

Provide a brief, friendly acknowledgment summarizing what wallet data was retrieved and displayed to the user. Be concise (1-2 sentences)."#,
            user_query, tool_results
        );

        let response = call_gemini_text(
            &ack_prompt,
            GeminiModel::GeminiLite,
            &self.config.gemini_api_key,
        )
        .await
        .map_err(|e| format!("Acknowledgment error: {:?}", e))?;

        Ok(response)
    }

    /// Process user input and return AI response
    pub async fn process_message(&self, user_input: &str) -> Result<String, String> {
        let response = call_gemini_text(
            user_input,
            GeminiModel::GeminiLite,
            &self.config.gemini_api_key,
        )
        .await
        .map_err(|e| format!("Gemini API error: {:?}", e))?;

        Ok(response)
    }

    /// Process chat message and return formatted response
    pub async fn process_chat_message(
        &self,
        chat_message: &ChatMessage,
    ) -> Result<ChatMessage, String> {
        let response_text = self.process_message(&chat_message.content).await?;

        let response = ChatMessage {
            id: Some(generate_id(Role::Assistant)),
            role: crate::models::Role::Assistant,
            content: response_text.clone(),
            name: None,
        };

        Ok(response)
    }

    /// Process message with function calling support - returns response with optional function call
    /// This matches the TypeScript flow: AI responds with text + function call, we parse both
    pub async fn process_message_with_functions(
        &self,
        user_input: &str,
    ) -> Result<(Option<String>, Option<String>), String> {
        // For now, use simple text processing (function calling will be added later)
        // This returns (text_response, function_name)
        let response_text = self.process_message(user_input).await?;
        Ok((Some(response_text), None))
    }

    pub async fn process_chat_message_orq(
        &self,
        chat_message: &ChatMessage
    ) -> Result<ChatMessage, String> {
        // 1. Build Context

        let system_prompt = format!("You are a teaching assistant operating inside a strict step-based orchestration system.

Topic: Integration

You MUST follow this teaching flow exactly, step by step.
You are currently on STEP {{CURRENT_STEP}}.

Teaching Flow:
STEP 1: Give a simple, intuitive introduction to the topic (no formulas, no depth).
STEP 2: Do a deeper explanation with core ideas and reasoning.
STEP 3: Provide a concise cheat sheet (key points, formulas, rules).
STEP 4: Ask ONE basic conceptual question to test understanding.
STEP 5: Stop. Do NOT continue further.

Rules:
- Execute ONLY the current step.
- Do NOT mention future steps.
- Do NOT repeat previous steps.
- Do NOT advance steps on your own.
- When you finish your step, output the exact token: <<STEP_DONE>>
- Do NOT output <<STEP_DONE>> unless the step is fully completed.

If the step is STEP 4, end by asking the question, then output <<STEP_DONE>>.

You do not control flow. The system controls flow.


- Chat History to which you have to answer the last message: chat_array


");

        // 2. Identify Current Status
        // 3. Generate Response Based on next step and user query
        println!("{:?}", system_prompt);

        let response_text = self.process_message(&system_prompt).await?;

        let response = ChatMessage {
            id: Some(generate_id(Role::System)),
            role: Role::Assistant,
            content: response_text.clone(),
            name: None,
        };

        Ok(response)
        

        
    }
}
