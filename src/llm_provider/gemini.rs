use std::vec;
use serde::{Deserialize, Serialize};
use crate::llm_provider::{functions::{self, intent_classifier, plan_todos_function, weather_function}, traits::{LlmConfig, LlmError, LlmProvider, LlmResponse}};

// Supported Gemini models
#[derive(Debug, Clone, PartialEq)]
pub enum GeminiModel {
    GeminiPro,
    GeminiLite,
    GeminiMed,
    Custom(String), // For future model support
}

#[derive(Serialize)]
pub struct  FunctionDeclaration {
    name: String, 
    description: String,
    parameters: FunctionParameters
}

#[derive(Serialize)]
pub struct FunctionParameters {
    #[serde(rename = "type")]
    parameter_type: String,
    properties: serde_json::Value,
    required: Vec<String>,
}

#[derive(Serialize)]
struct Tool {
    function_declarations: Vec<FunctionDeclaration>
}

impl GeminiModel {
    pub fn model_name(&self) -> String {
        match self {
            GeminiModel::GeminiPro => "gemini-2.5-pro".to_string(),
            GeminiModel::GeminiMed => "gemini-2.5-flash".to_string(),
            GeminiModel::GeminiLite => "gemini-2.5-flash-lite".to_string(),
            GeminiModel::Custom(name) => name.clone(),
        }
    }
    
    pub fn endpoint_url(&self) -> String {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", self.model_name())
    }
}
// Request/Response structures for Gemini API
#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    generation_config: GenerationConfig,
    tools: Option<Vec<Tool>>
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    temperature: f32,
    top_k: i32,
    top_p: f32,
    max_output_tokens: i32,
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponse {
    pub candidates: Vec<Candidate>,
}

#[derive(Deserialize, Debug)]
pub struct Candidate {
    pub content: ContentResponse,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ContentResponse {
    pub parts: Vec<PartResponse>,
}

#[derive(Deserialize, Debug)]
pub struct PartResponse {
    pub text: Option<String>,
    #[serde(rename = "functionCall")]
    pub function_call: Option<FunctionCallResponse>
}


#[derive(Deserialize, Debug, Serialize)]
pub enum Available_Functions {
    #[serde(rename = "plan_todos")]
    PlanTodos,
    #[serde(rename = "get_weather")]
    GetWeather
}

impl ToString for Available_Functions {
    fn to_string(&self) -> String {
        match self {
            Available_Functions::PlanTodos => "plan_todos".to_string(),
            Available_Functions::GetWeather => "get_weather".to_string()
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct FunctionCallResponse {
    pub name: Available_Functions,
    pub args: serde_json::Value,
}

pub fn create_function_declaration(
    name: &str,
    description:&str,
    parameters: serde_json::Value,
    required: Vec<String>
) -> FunctionDeclaration {
    FunctionDeclaration { name: name.to_string(), description: description.to_string(), 
        parameters: FunctionParameters {
            parameter_type: "object".to_string(),
            properties: parameters,
            required 
        } 
    }

}
// Convert GeminiError to LlmError
impl From<GeminiError> for LlmError {
    fn from(err: GeminiError) -> Self {
        match err {
            GeminiError::RequestFailed(msg) => LlmError::RequestFailed(msg),
            GeminiError::InvalidResponse(msg) => LlmError::InvalidResponse(msg),
            GeminiError::MissingApiKey => LlmError::MissingApiKey,
        }
    }
}

// Legacy GeminiError for backward compatibility
#[derive(Debug)]
pub enum GeminiError {
    RequestFailed(String),
    InvalidResponse(String),
    MissingApiKey,
}

impl std::fmt::Display for GeminiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
            GeminiError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            GeminiError::MissingApiKey => write!(f, "Missing API key"),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct PlanResult {
    pub todos: Vec<Todo>
}

#[derive(Deserialize, Debug)]
pub struct Todo {
    pub task: String,
    pub description: String,
    #[serde(default)]  // Add this
    pub dependencies: Vec<String>,
    #[serde(default)]  // Add this
    pub estimated_time: f64,
}

impl Default for PlanResult {
    fn default() -> Self {
        Self {
            todos: vec![]
        }
    }
}

pub async fn call_gemini_api(
    prompt: &str,
    model: GeminiModel,
    api_key: &str,
    use_functions: bool
) -> Result<GeminiResponse, GeminiError> {
    // Validate API key
    if api_key.is_empty() {
        return Err(GeminiError::MissingApiKey);
    }

    let available_functions = vec![
        weather_function(),
        plan_todos_function()
    ];

    // Build the request
    let mut request = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
        generation_config: GenerationConfig {
            temperature: 0.7,
            top_k: 40,
            top_p: 0.95,
            max_output_tokens: 1024,
        },
        tools: None
    };

    if use_functions {
        request.tools = Some(vec![
            Tool {
                function_declarations: available_functions
            }
        ])
    }

   

    // Create HTTP client
    let client = reqwest::Client::new();
    
    // Build URL with API key using selected model
    let url = format!("{}?key={}", model.endpoint_url(), api_key);

    // println!("[REQUEST] Body: {}", serde_json::to_string_pretty(&request).unwrap_or_else(|_| "Failed to serialize".to_string()));

    // Make the request
    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| GeminiError::RequestFailed(e.to_string()))?;

    // Check if request was successful
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(GeminiError::RequestFailed(format!(
            "API request failed with status {}: {}",
            status,
            error_text
        )));
    }

    // Parse response
    let gemini_response: GeminiResponse = response
        .json()
        .await
        .map_err(|e| GeminiError::InvalidResponse(e.to_string()))?;


    // Handle both text and function call responses
    Ok(gemini_response)
}

// In gemini.rs, add this new function
pub async fn plan_from_query(
    user_query: &str,
    model: GeminiModel,
    api_key: &str,
) -> Result<PlanResult, GeminiError> {
    if api_key.is_empty() {
        return Err(GeminiError::MissingApiKey);
    }

    let request = serde_json::json!({
        "contents": [
            {
                "parts": [
                    {
                        "text": format!("Analyze this user query and create an execution plan. The plan should identify if this is a simple request (normal) or complex request (complex). Then generate a list of todos that need to be executed to fulfill the request. User query: {}", user_query)
                    }
                ]
            }
        ],
        "generation_config": {
            "response_mime_type": "application/json",
            "response_schema": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "List of tasks to execute",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task": {
                                    "type": "string",
                                    "description": "Short task name"
                                },
                                "description": {
                                    "type": "string",
                                    "description": "Detailed description of what needs to be done"
                                },
                                "dependencies": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "List of task IDs this task depends on"
                                },
                                "estimated_time": {
                                    "type": "number",
                                    "description": "Estimated time in seconds"
                                }
                            },
                            "required": ["task", "description"]
                        }
                    }
                },
                "required": ["todos"]
            }
        }
    });

    let client = reqwest::Client::new();
    let url = format!("{}?key={}", model.endpoint_url(), api_key);

    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| GeminiError::RequestFailed(e.to_string()))?;

    

    let status = response.status();

    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(GeminiError::RequestFailed(format!(
            "API request failed with status {}: {}",
            status, error_text
        )));
    }

    let gemini_response: GeminiResponse = response
        .json()
        .await
        .map_err(|e| GeminiError::InvalidResponse(e.to_string()))?;

    let text_content = gemini_response
        .candidates
        .first()
        .and_then(|candidate| candidate.content.parts.first())
        .and_then(|part| part.text.as_ref())
        .ok_or_else(|| GeminiError::InvalidResponse("No text content in response".to_string()))?;

    // println!("{:#?}", text_content);

    let plan: PlanResult = serde_json::from_str(text_content)
        .map_err(|e| GeminiError::InvalidResponse(format!("Failed to parse JSON: {}", e)))?;

    Ok(plan)
}



