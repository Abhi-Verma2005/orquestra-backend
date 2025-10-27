// LLM Provider Traits for Orchestrator Integration
use serde::{Deserialize, Serialize};

// Common error type for all LLM providers
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("Request failed: {0}")]
    RequestFailed(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Missing API key")]
    MissingApiKey,
    #[error("Model not supported: {0}")]
    ModelNotSupported(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

// Generic response type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub model_used: String,
    pub tokens_used: Option<u32>,
    pub finish_reason: Option<String>,
}

// Configuration trait for LLM providers
pub trait LlmConfig {
    fn model_name(&self) -> &str;
    fn temperature(&self) -> f32;
    fn max_tokens(&self) -> u32;
}

// Main LLM provider trait
#[async_trait::async_trait]
pub trait LlmProvider {
    type Config: LlmConfig + Send + Sync;
    
    /// Generate a response from the LLM
    async fn generate(&self, prompt: &str, config: &Self::Config) -> Result<LlmResponse, LlmError>;
    
    /// Get available models for this provider
    fn available_models(&self) -> Vec<String>;
    
    /// Validate configuration
    fn validate_config(&self, config: &Self::Config) -> Result<(), LlmError>;
    
    /// Get provider name
    fn provider_name(&self) -> &str;
}

// Orchestrator-friendly interface
pub struct LlmRequest {
    pub prompt: String,
    pub model: Option<String>, // None means use default
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmRequestBuilder {
    prompt: String,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
}

impl LlmRequestBuilder {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            model: None,
            temperature: None,
            max_tokens: None,
        }
    }
    
    pub fn model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }
    
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
    
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
    
    pub fn build(self) -> LlmRequest {
        LlmRequest {
            prompt: self.prompt,
            model: self.model,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        }
    }
}

impl LlmRequest {
    pub fn builder(prompt: String) -> LlmRequestBuilder {
        LlmRequestBuilder::new(prompt)
    }
}
