use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Function,  // Add this for function results
    Data,
    Tool
}

impl ToString for Role {
    fn to_string(&self) -> String {
        match self {
            Role::System => "system".to_string(),
            Role::User => "user".to_string(),
            Role::Assistant => "assistant".to_string(),
            Role::Function => "function".to_string(),
            Role::Data => "data".to_string(),
            Role::Tool => "tool".to_string(),
        }
    }
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: Role,
    pub content: String,
    pub name: Option<String>, // For function names
}

pub fn messages_to_prompt(messages: &Vec<ChatMessage>) -> String {
    messages.iter().map(|message| {
        let role_str = message.role.to_string();
        if let Some(name) = &message.name {
            format!("{} ({}): {}", role_str, name, message.content)
        } else {
            format!("{}: {}", role_str, message.content)
        }
    }).collect::<Vec<String>>().join("\n")
}
