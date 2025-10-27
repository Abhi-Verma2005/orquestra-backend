// Configuration loading and settings
use std::env;
use dotenvy;

#[derive(Debug, Clone)]
pub struct Config {
    pub gemini_api_key: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        // Load .env file if it exists
        dotenvy::dotenv().ok(); // Use .ok() to make it optional
        
        Ok(Config {
            gemini_api_key: env::var("GOOGLE_GENERATIVE_AI_API_KEY")
                .expect("API key is required")
        })
    }
}