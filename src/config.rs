// Configuration loading and settings
use std::env;
use dotenvy;

#[derive(Debug, Clone)]
pub struct Config {
    pub gemini_api_key: String,
    pub solana_rpc_url: String,
    pub ethereum_rpc_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        // Load .env file if it exists
        dotenvy::dotenv().ok(); // Use .ok() to make it optional
        
        Ok(Config {
            gemini_api_key: env::var("GOOGLE_GENERATIVE_AI_API_KEY").unwrap_or_default(),
            solana_rpc_url: env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            ethereum_rpc_url: env::var("ETHEREUM_RPC_URL")
                .unwrap_or_else(|_| "https://eth.llamarpc.com".to_string()),
        })
    }
}