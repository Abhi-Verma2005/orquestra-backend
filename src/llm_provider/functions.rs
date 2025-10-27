use serde::{Deserialize, Serialize};
use serde_json;

use crate::llm_provider::gemini::{create_function_declaration, FunctionDeclaration};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IntentType {
    Normal,
    Complex,
    Unknown
}

impl IntentType {
    pub fn as_str(&self) -> &str {
        match self {
            IntentType::Normal => "normal",
            IntentType::Complex => "complex",
            IntentType::Unknown => "unknown"
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "normal" => IntentType::Normal,
            "complex" => IntentType::Complex,
            _ => IntentType::Unknown
        }
    }
}

// Static function declarations
pub fn intent_classifier() -> FunctionDeclaration {
    create_function_declaration(
        "classify_intent",
        "Classify whether user query is simple or requires complex tool execution",
        serde_json::json!({
            "intent_type": {
                "type": "string",
                "description": "Classification of the user's intent",
                "enum": ["normal", "complex", "unknown"]
            },
            "reasoning": {
                "type": "string", 
                "description": "Brief explanation of why this classification was made"
            },
            "confidence": {
                "type": "number",
                "description": "Confidence score from 0.0 to 1.0",
                "minimum": 0.0,
                "maximum": 1.0
            }
        }),
        vec!["intent_type".to_string(), "reasoning".to_string(), "confidence".to_string()]
    )
}

pub fn plan_todos_function() -> FunctionDeclaration {
    create_function_declaration(
        "plan_todos",
        "Plan the todos needed to fulfill the request",
        serde_json::json!({
            "todos": {
                "type": "array",
                "description": "List of todos needed to fulfill the request",
                "items": {
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "Short task name"
                        }
                    }
                }
            }
        }),
        vec!["todos".to_string()]
    )
}
// Add more function declarations here as needed
pub fn weather_function() -> FunctionDeclaration {
    create_function_declaration(
        "get_weather",
        "Get current weather information",
        serde_json::json!({
            "location": {
                "type": "string",
                "description": "City or location name"
            },
            "unit": {
                "type": "string",
                "description": "Temperature unit",
                "enum": ["celsius", "fahrenheit"]
            }
        }),
        vec!["location".to_string()]
    )
}