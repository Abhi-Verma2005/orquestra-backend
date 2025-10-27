// src/websocket/protocol.rs
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

// WebSocket message types
#[derive(Serialize, Deserialize, Debug)]
pub struct JoinRoomMessage {
    pub room_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendMessageData {
    pub room_id: String,
    pub message: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum MessageType {
    JoinRoom,
    SendMessage,
    LeaveRoom,
    JoinAck
}

impl ToString for MessageType {
    fn to_string(&self) -> String {
        match self {
            MessageType::JoinRoom => "joinRoom".to_string(),
            MessageType::LeaveRoom => "leaveRoom".to_string(),
            MessageType::SendMessage => "sendMessage".to_string(),
            MessageType::JoinAck => "joinAck".to_string()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebSocketMessage {
    #[serde(rename = "type")]
    pub message_type: MessageType,
    pub payload: serde_json::Value,
    pub timestamp: i64,
    pub message_id: String

}

// Room management types
#[derive(Clone, Debug)]
pub struct RoomMessage {
    pub room_id: String,
    pub content: String,
}

// Type aliases for WebSocket state management
pub type Tx = broadcast::Sender<RoomMessage>;
pub type _Rx = broadcast::Receiver<RoomMessage>;
pub type Clients = Arc<Mutex<HashMap<SocketAddr, Tx>>>;
pub type ClientRooms = Arc<Mutex<HashMap<SocketAddr, HashSet<String>>>>;
pub type Rooms = Arc<Mutex<HashMap<String, HashSet<SocketAddr>>>>;