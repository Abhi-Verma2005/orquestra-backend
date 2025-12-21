// src/websocket/protocol.rs
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::models::{ChatMessage, Role};

// WebSocket message types
#[derive(Serialize, Deserialize, Debug)]
pub struct JoinRoomMessage {
    pub chat_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>, // User's name or email for display
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SdkMessage {
    pub id: String,
    pub role: Role,
    pub content: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendMessageData {
    pub chat_id: String,
    pub message: RoomMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ai_message: Option<bool>, // Frontend can hint, but backend will verify
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_group_chat: Option<bool>, // Tell backend if this is a group chat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_addresses: Option<WalletAddresses>, // Optional wallet addresses from frontend
}


#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    #[serde(rename = "connection_established")]
    ConnectionEstablished,
    #[serde(rename = "connection_error")]
    ConnectionError,
    #[serde(rename = "join_chat")]
    JoinChat,
    #[serde(rename = "leave_chat")]
    LeaveChat,
    #[serde(rename = "chat_message")]
    ChatMessage,
    #[serde(rename = "message_received")]
    MessageReceived,
    #[serde(rename = "message_error")]
    MessageError,
    #[serde(rename = "text_stream")]
    TextStream,
    #[serde(rename = "text_stream_end")]
    TextStreamEnd,
    #[serde(rename = "function_call")]
    FunctionCall,
    #[serde(rename = "function_call_start")]
    FunctionCallStart,
    #[serde(rename = "function_call_end")]
    FunctionCallEnd,
    #[serde(rename = "function_result")]
    FunctionResult,
    #[serde(rename = "function_error")]
    FunctionError,
    #[serde(rename = "plan_created")]
    PlanCreated,
    #[serde(rename = "plan_updated")]
    PlanUpdated,
    #[serde(rename = "plan_completed")]
    PlanCompleted,
    #[serde(rename = "cart_updated")]
    CartUpdated,
    #[serde(rename = "cart_cleared")]
    CartCleared,
    #[serde(rename = "system_message")]
    SystemMessage,
    #[serde(rename = "heartbeat")]
    Heartbeat,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "user_message")]
    UserMessage, // For non-AI group messages
    #[serde(rename = "user_joined")]
    UserJoined, // Notification when user joins
    #[serde(rename = "user_left")]
    UserLeft, // Notification when user leaves
    #[serde(rename = "open_sidebar")]
    OpenSidebar, // Open the sidebar panel
    #[serde(rename = "wallet_data")]
    WalletData, // Send portfolio data
    #[serde(rename = "intent_detected")]
    IntentDetected, // Indicate intent detection result
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebSocketMessage {
    #[serde(rename = "type")]
    pub message_type: MessageType,
    pub payload: serde_json::Value,
    pub timestamp: i64,
    pub message_id: String

}

// Room management types
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RoomMessage {
    pub room_id: String,
    pub payload: ChatMessage,
}


// Chat member information
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMember {
    pub user_id: String,
    pub role: String, // "owner" | "member"
    pub joined_at: Option<i64>, // Timestamp
}

// Type aliases for WebSocket state management
// Use mpsc channels for per-client messaging (not broadcast) - each client gets its own channel
pub type ClientTx = mpsc::Sender<RoomMessage>;
pub type Clients = Arc<Mutex<HashMap<SocketAddr, ClientTx>>>;
// Keep Tx for backward compatibility (used in server.rs for now, will remove)
pub type Tx = broadcast::Sender<RoomMessage>;
pub type _Rx = broadcast::Receiver<RoomMessage>;
pub type ClientRooms = Arc<Mutex<HashMap<SocketAddr, HashSet<String>>>>;
pub type Rooms = Arc<Mutex<HashMap<String, HashSet<SocketAddr>>>>;
pub type UserClients = Arc<Mutex<HashMap<SocketAddr, String>>>; // Maps SocketAddr -> user_id

// Track which user_ids have joined each room (to prevent duplicate join notifications)
pub type RoomUserIds = Arc<Mutex<HashMap<String, HashSet<String>>>>; // Maps room_id -> Set<user_id>

// Store user_id -> user_name mapping for displaying names in messages
pub type UserNames = Arc<Mutex<HashMap<String, String>>>; // Maps user_id -> user_name

// Wallet address storage
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct WalletAddresses {
    pub solana: Option<String>,
    pub ethereum: Option<String>,
}

pub type WalletClients = Arc<Mutex<HashMap<SocketAddr, WalletAddresses>>>; // Maps SocketAddr -> wallet addresses