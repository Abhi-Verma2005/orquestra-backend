// OpenAI API integration
use serde::{Deserialize, Serialize};
use crate::llm_provider::traits::LlmError;

#[derive(Debug, Clone)]
pub enum OpenAIModel {
    Gpt4,
    Gpt4Turbo,
    Gpt35Turbo,
    Custom(String),
}

impl OpenAIModel {
    pub fn model_name(&self) -> String {
        match self {
            OpenAIModel::Gpt4 => "gpt-4".to_string(),
            OpenAIModel::Gpt4Turbo => "gpt-4-turbo-preview".to_string(),
            OpenAIModel::Gpt35Turbo => "gpt-3.5-turbo".to_string(),
            OpenAIModel::Custom(name) => name.clone(),
        }
    }
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
    max_tokens: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    error: Option<OpenAIError>,
}

#[derive(Deserialize, Debug)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAIError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

pub async fn call_openai_api(
    prompt: &str,
    model: OpenAIModel,
    api_key: &str,
) -> Result<String, LlmError> {
    let client = reqwest::Client::new();
    let url = "https://api.openai.com/v1/chat/completions";

    let request = OpenAIRequest {
        model: model.model_name(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
        temperature: 0.7,
        max_tokens: Some(1024),
    };

    println!("📡 [OpenAI] Calling API with model: {}", model.model_name());
    
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| LlmError::RequestFailed(format!("Request error: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(LlmError::RequestFailed(format!(
            "API request failed with status {}: {}",
            status, error_text
        )));
    }

    let api_response: OpenAIResponse = response
        .json()
        .await
        .map_err(|e| LlmError::InvalidResponse(format!("Failed to parse response: {}", e)))?;

    if let Some(error) = api_response.error {
        return Err(LlmError::RequestFailed(format!(
            "OpenAI API error: {}",
            error.message
        )));
    }

    if let Some(choice) = api_response.choices.first() {
        if let Some(finish_reason) = &choice.finish_reason {
            if finish_reason == "length" {
                println!("⚠️ [OpenAI] Response was truncated due to token limit");
            }
        }
        return Ok(choice.message.content.clone());
    }

    Err(LlmError::InvalidResponse("No choices in response".to_string()))
}
