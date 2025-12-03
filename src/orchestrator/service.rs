// src/orchestrator/service.rs
use crate::{
    config::Config,
    llm_provider::gemini::{call_gemini_api, call_gemini_text, GeminiModel, GeminiResponse},
    models::ChatMessage,
    services::{
        cache::CacheService,
        price_service::PriceService,
        metadata_service::MetadataService,
        solana_client::SolanaClient,
        ethereum_client::EthereumClient,
    },
};

pub struct OrchestratorService {
    config: Config,
    solana_client: SolanaClient,
    ethereum_client: EthereumClient,
}

impl OrchestratorService {
    pub fn new(config: Config) -> Self {
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
        println!("🔍 [Orchestrator] Detecting wallet intent for: {}", user_input);
        
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
        ).await.map_err(|e| {
            println!("❌ [Orchestrator] Intent detection API error: {:?}", e);
            format!("Intent detection error: {:?}", e)
        })?;

        let intent_needed = response.trim().to_lowercase().contains("yes");
        println!("✅ [Orchestrator] Wallet intent detected: {}", intent_needed);
        
        Ok(intent_needed)
    }

    /// Generate acknowledgment message after tool execution
    pub async fn generate_acknowledgment(
        &self,
        user_query: &str,
        tool_results: &str,
    ) -> Result<String, String> {
        println!("💬 [Orchestrator] Generating acknowledgment...");
        
        let ack_prompt = format!(
            r#"The user asked: "{}"

I fetched their wallet data. Here's what I found:
{}

Provide a brief, friendly acknowledgment summarizing what wallet data was retrieved and displayed to the user. Be concise (1-2 sentences)."#,
            user_query,
            tool_results
        );

        let response = call_gemini_text(
            &ack_prompt,
            GeminiModel::GeminiLite,
            &self.config.gemini_api_key,
        ).await.map_err(|e| {
            println!("❌ [Orchestrator] Acknowledgment API error: {:?}", e);
            format!("Acknowledgment error: {:?}", e)
        })?;

        println!("✅ [Orchestrator] Generated acknowledgment");
        Ok(response)
    }

    /// Process user input and return AI response
    pub async fn process_message(&self, user_input: &str) -> Result<String, String> {
        println!("🤖 [Orchestrator] Processing user input: {}", user_input);
        
        // Simple prompt - just send user input to AI
        println!("📡 [Orchestrator] Calling Gemini API...");
        let response = call_gemini_text(
            user_input,
            GeminiModel::GeminiLite,
            &self.config.gemini_api_key,
        ).await.map_err(|e| {
            println!("❌ [Orchestrator] Gemini API error: {:?}", e);
            format!("Gemini API error: {:?}", e)
        })?;

        println!("✅ [Orchestrator] Received response from Gemini API");
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
}
