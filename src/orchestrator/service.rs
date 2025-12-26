use std::{collections::{hash_map::Entry, HashMap}, sync::Arc};

// use anyhow::{anyhow, Ok};
use axum::http::header::{OccupiedEntry, VacantEntry};
use tokio::sync::Mutex;

// src/orchestrator/service.rs
use crate::{
    config::Config, db::service::ChatDbService, llm_provider::gemini::{call_gemini_text, GeminiModel}, models::{ChatMessage, Role}, services::{
        cache::CacheService, metadata_service::MetadataService,
        price_service::PriceService,
    }, utils::gen_id::generate_id, websocket::{handler::{broadcast_to_room, stream_text_to_room}, Clients, Rooms, UserClients}
};

pub enum TeachingStep {
    Introduction,
    Explanation,
    CheatSheet,
    Question,
    Done
}

impl From<String> for TeachingStep{
    fn from(item: String) -> Self {
        match item.as_str() {
            "introduction" => TeachingStep::Introduction,
            "explanation" => TeachingStep::Explanation,
            "cheat_sheet" => TeachingStep::CheatSheet,
            "question" => TeachingStep::Question,
            "done" => TeachingStep::Done,
            _ => TeachingStep::Introduction
        }
    }
}

pub struct SessionState {
    pub chat_id: String,
    pub user_id: String,
    pub topic: String,
    pub current_step: TeachingStep
}

impl SessionState{
    pub fn new(chat_id: String, user_id: String, topic: String, current_step: Option<TeachingStep>) -> Self {
        Self {
            chat_id,
            user_id,
            topic,
            current_step: current_step.unwrap_or_else(|| TeachingStep::Introduction)
        }
    }

    // pub fn cloned() -> Self {
    //     Self {
    //         chat_id
    //     }
    // }
}

type SessionRegistry = Arc<Mutex<HashMap<String, Arc<Mutex<SessionState>>>>>;

pub struct OrchestratorService {
    config: Config,
    chat_db: ChatDbService,
    pub session_registry: SessionRegistry
}

impl OrchestratorService {
    pub fn new(config: Config, chat_db: ChatDbService) -> Self {
        // Initialize blockchain services
        let cache = CacheService::new();
        let session_registry: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));

        Self {
            config,
            chat_db,
            session_registry
        }
    }

    pub async fn make_safe_entry(&self, chat_id: &String, user_id: &String) {
        // 1. Check for cache
        let mut guard = self.session_registry.lock().await;
        guard.entry(chat_id.clone()).or_insert( {
            // check for state in db
            // if 
            let result = self.chat_db.read_or_update_session_state(&chat_id, &user_id, "integration").await;
            match result {
                Ok(val) => {
                    let current_step = TeachingStep::from(val.current_step);
                    let session_entry = SessionState::new(chat_id.clone(), user_id.clone(), String::from("integration"), Some(current_step));
                    Arc::new(Mutex::new(session_entry))
                },
                Err(e) => {
                    eprintln!("Got error while reading and updating session state to db: {:?}", e);
                    let session_entry = SessionState::new(chat_id.clone(), user_id.clone(), String::from("integration"), None);
                    Arc::new(Mutex::new(session_entry))
                }
            }

        });
        // 2. No Cache ? Check DB
        // 3. No DB state ? Probably a new chat hence create a new state and update cache and return it.
        // 4. If DB state return
        // 5. Cache exists early return
        
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
        println!("reached process chat message");
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
        chat_id: &String,
        user_id: &String,
        chat_message: &ChatMessage,
        clients: &Clients,
        rooms: &Rooms,
        user_clients: &UserClients,
    ) -> Result<ChatMessage, String> {
        // 1. Build Context
        let state = {
            let guard = self.session_registry.lock().await;
            guard.get(chat_id).cloned()
        };

        let msg_to_save = ChatMessage {
            id: Some(generate_id(Role::User)),
            ..chat_message.clone()
        };

        let usr_msg = vec![msg_to_save];

        // Get session state from db
        
        let st = state.ok_or("Session State is None")?;
        let final_st = st.lock().await;
        let msg = serde_json::json!(usr_msg);
        // let chat_history_check = self.chat_db.get_chat_messages(chat_id).await;
        // eprintln!("Check Chat history: {:?}", chat_history_check);
            let saved = self.chat_db.save_chat(chat_id, user_id, &msg, &final_st.topic, &final_st.topic, false).await;
            match saved {
                Ok(()) => {
                    
                    let mut i = 0;
                    while i < 5 {
                        let chat_history_result = self.chat_db.get_chat_messages(chat_id).await;

                        match chat_history_result {
                            Ok(chat_history) => {
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
            
            
                                - Chat History to which you have to answer the last message: {:?}
            
            
                                ", chat_history);
                                let response_text = self.process_message(&system_prompt).await?;
                                let ai_message = ChatMessage {
                                    id: Some(generate_id(Role::Assistant)),
                                    role: Role::Assistant,
                                    content: response_text.clone(),
                                    name: Some(String::from("assisstant"))
                                };
                                let saved_ai_message = self.chat_db.save_chat(chat_id, user_id, &serde_json::json!(vec![ai_message]), &final_st.topic, &final_st.topic, false).await;
            
                                match saved_ai_message {
                                    Ok(()) => {
                                        // eprintln!("Saved AI response: {:?}", response_text);
                                    },
                                    Err(e) => {
                                        eprintln!("Error While saving ai message in db: {:?}", e);
                                    }
                                };
                        
                                let response = ChatMessage {
                                    id: Some(generate_id(Role::System)),
                                    role: Role::Assistant,
                                    content: response_text.clone(),
                                    name: None,
                                };
                                stream_text_to_room(
                                    &chat_id,
                                    &response.content,
                                    &clients,
                                    &rooms,
                                    &user_clients,
                                ).await;
                            },
                            Err(e) => {
                                eprintln!("Encountered error: {:?}", e);
                            }
                        }
                        // 2. Identify Current Status
                        // 3. Generate Response Based on next step and user query
                        i += 1;
                    };

                    let response = ChatMessage {
                        id: Some(generate_id(Role::System)),
                        role: Role::Assistant,
                        content: String::from("Stream Done"),
                        name: None,
                    };
                    Ok(response)


                },
                Err(e) => {
                    eprintln!("Error While saving message in db: {:?}", e);
                    Err(String::from("Message not saved in db, cannot process this message further"))
                }
            }
                
        
        // match state {
        //     Option::Some(st) => {
                
        //     Option::None => {
        //         eprintln!("Session State is None, cannot process this message further");
        //         Err(String::from("Session State is None, cannot process this message further"))
        //     }
        // }
        
    }
}
