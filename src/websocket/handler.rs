// src/websocket/handler.rs
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use crate::orchestrator::service::OrchestratorService;
use crate::websocket::protocol::{
    Clients, ClientRooms, Rooms, RoomMessage, Tx, WebSocketMessage, JoinRoomMessage, SendMessageData, UserClients
};
use crate::websocket::MessageType;

pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    tx: Tx,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    orchestrator: Arc<OrchestratorService>,
) {
    println!("Client connected: {}", addr);
    
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("❌ Failed WebSocket handshake from {}: {:?}", addr, e);
            return;
        }
    };
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let mut rx: broadcast::Receiver<RoomMessage> = tx.subscribe();
    
    // Register client
    {
        let mut clients_lock = clients.lock().await;
        clients_lock.insert(addr, tx.clone());
    }

    let welcome_msg = WebSocketMessage {
        message_type: MessageType::ConnectionEstablished,
        payload: json!({
            "message": "Welcome to AI Orchestrator WebSocket server!"
        }),
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        message_id: format!("msg_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis())
    };
    
    let welcome_json = serde_json::to_string(&welcome_msg).unwrap();
    println!("📤 [{}] Sending welcome message: {}", addr, welcome_json);
    match ws_sender.send(Message::Text(welcome_json)).await {
        Ok(_) => println!("✅ [{}] Welcome message sent successfully", addr),
        Err(e) => println!("❌ [{}] Failed to send welcome message: {:?}", addr, e),
    }
    
    // Handle outgoing messages in a separate task
    let mut ws_sender_clone = ws_sender;
    let addr_for_tx = addr.clone();
    let tx_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Serialize the entire message as JSON
            if let Ok(json_str) = serde_json::to_string(&msg) {
                println!("📤 [{}] Broadcasting message to client: {}", addr_for_tx, json_str);
                if ws_sender_clone.send(Message::Text(json_str)).await.is_err() {
                    println!("❌ [{}] Failed to send broadcast message, closing connection", addr_for_tx);
                    break;
                } else {
                    println!("✅ [{}] Broadcast message sent successfully", addr_for_tx);
                }
            } else {
                println!("❌ [{}] Failed to serialize broadcast message", addr_for_tx);
            }
        }
    });
    
    // Handle incoming messages in the current task
    let addr_clone2 = addr.clone();
    let clients_clone = clients.clone();
    let rooms_clone = rooms.clone();
    let client_rooms_clone = client_rooms.clone();
    let user_clients_clone = user_clients.clone();
    let orchestrator_clone = orchestrator.clone();
    
    // Process incoming messages (not spawned - handles Send issue)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                println!("📥 [{}] Received text message: {}", addr_clone2, text);
                match serde_json::from_str::<WebSocketMessage>(&text) {
                    Ok(parsed) => {
                        println!("✅ [{}] Successfully parsed message type: {:?}", addr_clone2, parsed.message_type);
                        handle_websocket_message(
                            parsed,
                            addr_clone2,
                            clients_clone.clone(),
                            rooms_clone.clone(),
                            client_rooms_clone.clone(),
                            user_clients_clone.clone(),
                            orchestrator_clone.clone(),
                        ).await;
                    }
                    Err(e) => {
                        println!("❌ [{}] Failed to parse WebSocket message: {} | Raw text: {}", addr_clone2, e, text);
                    }
                }
            }
            Ok(Message::Frame(_)) => {
                println!("📦 [{}] Received frame message", addr_clone2);
            }
            Ok(Message::Close(_)) => {
                println!("🔌 [{}] Received close message", addr_clone2);
                break;
            }
            Ok(Message::Ping(_)) => {
                println!("🏓 [{}] Received ping", addr_clone2);
                // WebSocket will automatically respond with pong
            }
            Ok(Message::Pong(_)) => {
                println!("🏓 [{}] Received pong", addr_clone2);
            }
            Ok(Message::Binary(_)) => {
                println!("📦 [{}] Received binary message", addr_clone2);
            }
            Err(e) => {
                println!("❌ [{}] WebSocket error: {:?}", addr_clone2, e);
                break;
            }
        }
    }
    
    // Cancel the outgoing task when we're done
    tx_task.abort();
    
    // Cleanup on disconnect
    cleanup_client(addr, clients, rooms, client_rooms, user_clients).await;
    println!("Client disconnected: {}", addr);
}

async fn handle_websocket_message(
    message: WebSocketMessage,
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    orchestrator: Arc<OrchestratorService>,
) {
    println!("🔍 [{}] Handling message type: {:?}", addr, message.message_type);
    println!("📋 [{}] Message payload: {}", addr, serde_json::to_string(&message.payload).unwrap_or_else(|_| "Failed to serialize".to_string()));
    
    match message.message_type {
        MessageType::JoinChat => {
            println!("🚪 [{}] Processing JoinChat message", addr);
            match serde_json::from_value::<JoinRoomMessage>(message.payload) {
                Ok(join_data) => {
                    println!("✅ [{}] JoinChat parsed successfully, chat_id: {}, user_id: {:?}", 
                        addr, join_data.chat_id, join_data.user_id);
                    
                    // Store user_id mapping if provided
                    if let Some(user_id) = join_data.user_id.clone() {
                        let mut user_clients_lock = user_clients.lock().await;
                        user_clients_lock.insert(addr, user_id.clone());
                        println!("✅ [{}] Stored user_id: {}", addr, user_id);
                    }
                    
                    join_room(addr, join_data.chat_id.clone(), rooms.clone(), client_rooms.clone(), user_clients.clone(), clients.clone()).await;
                }
                Err(e) => {
                    println!("❌ [{}] Failed to parse JoinChat payload: {}", addr, e);
                }
            }
        }
        MessageType::ChatMessage => {
            println!("💬 [{}] Processing ChatMessage", addr);
            let payload_clone = message.payload.clone();
            match serde_json::from_value::<SendMessageData>(message.payload) {
                Ok(send_data) => {
                    println!("✅ [{}] ChatMessage parsed successfully, chat_id: {}, message content: {}", 
                        addr, send_data.chat_id, send_data.message.payload.content);
                    
                    // Store user_id if provided in message
                    if let Some(user_id) = send_data.user_id.clone() {
                        let mut user_clients_lock = user_clients.lock().await;
                        user_clients_lock.insert(addr, user_id);
                    }
                    
                    handle_chat_message(addr, send_data, clients.clone(), rooms.clone(), user_clients.clone(), orchestrator).await;
                }
                Err(e) => {
                    println!("❌ [{}] Failed to parse ChatMessage payload: {} | Payload: {}", 
                        addr, e, serde_json::to_string(&payload_clone).unwrap_or_else(|_| "Failed to serialize".to_string()));
                }
            }
        }
        MessageType::LeaveChat => {
            println!("🚪 [{}] Processing LeaveChat message (not implemented yet)", addr);
        }
        _ => {
            println!("⚠️ [{}] Unknown message type: {:?}", addr, message.message_type);
        }
    }
}

async fn join_room(
    addr: SocketAddr,
    room_id: String,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    clients: Clients,
) {
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    {
        let mut rooms_lock = rooms.lock().await;
        let entry = rooms_lock.entry(room_id.clone()).or_default();
        let is_new_join = !entry.contains(&addr);
        entry.insert(addr);
        let participant_count_after = entry.len();
        
        // Only send UserJoined notification if:
        // 1. This is a new join (not already in room)
        // 2. User ID is available
        // 3. There are multiple participants (it's actually a group chat)
        // This prevents notifications on home page or when first person joins
        if is_new_join && user_id.is_some() && participant_count_after > 1 {
            let user_id_str = user_id.as_ref().unwrap().clone();
            let clients_lock = clients.lock().await;
            
            // Send notification to all existing participants (excluding the new joiner)
            for participant_addr in entry.iter() {
                if *participant_addr != addr {
                    if let Some(participant_tx) = clients_lock.get(participant_addr) {
                        let system_message = crate::models::ChatMessage {
                            role: crate::models::Role::System,
                            content: format!("User {} joined the chat", user_id_str),
                            name: Some("system".to_string()),
                        };
                        let room_message = RoomMessage {
                            room_id: room_id.clone(),
                            payload: system_message,
                        };
                        let _ = participant_tx.send(room_message);
                    }
                }
            }
        }
    }

    {
        let mut client_rooms_lock = client_rooms.lock().await;
        let entry = client_rooms_lock.entry(addr).or_default();
        entry.insert(room_id.clone());
    }
    
    if let Some(uid) = user_id {
        println!("{} (user: {}) joined the room {}", addr, uid, room_id);
    } else {
    println!("{} joined the room {}", addr, room_id);
    }
}

async fn handle_chat_message(
    addr: SocketAddr,
    chat_data: SendMessageData,
    clients: Clients,
    rooms: Rooms,
    user_clients: UserClients,
    orchestrator: Arc<OrchestratorService>,
) {
    println!("🤖 [{}] Processing chat message for room: {}", addr, chat_data.chat_id);
    println!("📝 [{}] User message content: {}", addr, chat_data.message.payload.content);
    
    // Get user_id for this connection
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    let content = chat_data.message.payload.content.trim();
    
    // Check if this is a group chat (from frontend flag)
    let is_group_chat = chat_data.is_group_chat.unwrap_or(false);
    
    // Message routing logic:
    // - If NOT group chat: route all messages to AI (existing behavior)
    // - If IS group chat: check for "@ai" prefix
    let has_ai_prefix = content.to_lowercase().starts_with("@ai");
    let should_route_to_ai = !is_group_chat || has_ai_prefix;
    
    if should_route_to_ai {
        // For group chats, broadcast the user's message first so everyone sees what was asked
        if is_group_chat {
            println!("💬 [{}] Broadcasting user's @ai message to group first", addr);
            let user_message = crate::models::ChatMessage {
                role: crate::models::Role::User,
                content: content.to_string(), // Original message with @ai prefix
                name: user_id.clone(),
            };
            broadcast_to_room(
                &chat_data.chat_id,
                user_message,
                clients.clone(),
                rooms.clone(),
            ).await;
        }
        
        // Remove "@ai" prefix (case-insensitive) and trim
        let mut final_content = content
            .trim_start()
            .chars()
            .skip_while(|c| c.is_whitespace())
            .skip_while(|c| c.to_lowercase().to_string() == "@")
            .skip_while(|c| c.to_lowercase().to_string() == "a" || c.to_lowercase().to_string() == "i")
            .skip_while(|c| c.is_whitespace())
            .collect::<String>()
            .trim()
            .to_string();
        
        // If content is empty after removing @ai, use a default message
        if final_content.is_empty() {
            final_content = "Hello".to_string();
        }
        
        // Create message for AI with cleaned content
        let mut ai_message = chat_data.message.payload.clone();
        ai_message.content = final_content;
    
    // Process the message through the orchestrator
        println!("🔄 [{}] Sending message to orchestrator (AI mode)...", addr);
        match orchestrator.process_chat_message(&ai_message).await {
        Ok(ai_response) => {
            println!("✅ [{}] Orchestrator returned response: {}", addr, ai_response.content);
            
            // Broadcast the AI response to all participants in the room
                broadcast_to_room(
                    &chat_data.chat_id,
                    ai_response,
                    clients.clone(),
                    rooms.clone(),
                ).await;
            }
            Err(e) => {
                eprintln!("❌ [{}] Error processing chat message: {}", addr, e);
                send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
            }
        }
    } else if is_group_chat {
        // Regular group message - broadcast to all participants (only if group chat)
        println!("💬 [{}] Broadcasting as group message (non-AI)", addr);
        
        let group_message = crate::models::ChatMessage {
            role: crate::models::Role::User,
            content: content.to_string(),
            name: user_id.clone(), // Include sender's user_id
        };
        
        // Broadcast to all participants including sender (so everyone sees it in same order)
        broadcast_to_room(
            &chat_data.chat_id,
            group_message,
            clients.clone(),
            rooms.clone(),
        ).await;
        
        // Also save the message to database immediately
        // Note: Each client will also try to save, but this ensures it's saved at least once
    } else {
        // Not a group chat and no "@ai" prefix - this shouldn't happen with correct frontend logic
        // But fallback: route to AI anyway (existing behavior)
        println!("⚠️ [{}] Message without @ai in non-group chat, routing to AI as fallback", addr);
        
        let ai_message = chat_data.message.payload.clone();
        match orchestrator.process_chat_message(&ai_message).await {
            Ok(ai_response) => {
                broadcast_to_room(
                    &chat_data.chat_id,
                    ai_response,
                    clients.clone(),
                    rooms.clone(),
                ).await;
            }
            Err(e) => {
                eprintln!("❌ [{}] Error processing chat message: {}", addr, e);
                send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
            }
        }
    }
}

// Helper function to broadcast messages to all room participants
async fn broadcast_to_room(
    room_id: &str,
    message: crate::models::ChatMessage,
    clients: Clients,
    rooms: Rooms,
) {
            let rooms_lock = rooms.lock().await;
    if let Some(participants) = rooms_lock.get(room_id) {
        println!("📢 Broadcasting to {} participants in room {}", participants.len(), room_id);
        println!("📢 Message content: {} | Sender: {:?}", 
            message.content.chars().take(50).collect::<String>(), 
            message.name);
        let clients_lock = clients.lock().await;
                
                let mut sent_count = 0;
        let mut failed_count = 0;
                for participant_addr in participants {
            if let Some(participant_tx) = clients_lock.get(participant_addr) {
                        let room_message = RoomMessage {
                    room_id: room_id.to_string(),
                    payload: message.clone(),
                        };
                        match participant_tx.send(room_message) {
                            Ok(_) => {
                                sent_count += 1;
                        println!("✅ Sent to participant: {}", participant_addr);
                            }
                            Err(e) => {
                        failed_count += 1;
                        println!("❌ Failed to send to participant {}: {:?}", participant_addr, e);
                            }
                        }
            } else {
                failed_count += 1;
                println!("⚠️ No sender found for participant: {}", participant_addr);
            }
        }
        println!("📤 Broadcast complete: {}/{} participants in room {} ({} failed)", 
            sent_count, participants.len(), room_id, failed_count);
    } else {
        println!("⚠️ No participants found in room: {}", room_id);
    }
}

async fn send_error_message(
    addr: SocketAddr,
    chat_id: &str,
    error_msg: &str,
    clients: Clients,
    _rooms: Rooms,
) {
    println!("📤 [{}] Sending error message to room: {}", addr, chat_id);
    
    // Create error chat message with a unique identifier to prevent duplicates
    let error_content = format!("Error: {}", error_msg);
    let error_chat_message = crate::models::ChatMessage {
        role: crate::models::Role::Assistant,
        content: error_content.clone(),
        name: Some("system".to_string()),
    };
    
    // Only send error to the sender (the client that sent the message), not broadcast to all
    // This prevents duplicate error messages when multiple clients are in the same room
    let client_lock = clients.lock().await;
    if let Some(sender_tx) = client_lock.get(&addr) {
        let room_message = RoomMessage {
            room_id: chat_id.to_string(),
            payload: error_chat_message,
        };
        match sender_tx.send(room_message) {
            Ok(_) => {
                println!("✅ [{}] Sent error message to sender", addr);
            }
            Err(e) => {
                println!("❌ [{}] Failed to send error to sender: {:?}", addr, e);
            }
        }
    } else {
        println!("⚠️ [{}] No sender found for error message", addr);
    }
}

async fn cleanup_client(
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
) {
    // Get user_id before removing
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    {
        let mut clients_lock = clients.lock().await;
        clients_lock.remove(&addr);
    }
    
    {
        let mut user_clients_lock = user_clients.lock().await;
        user_clients_lock.remove(&addr);
    }

    {
        let mut client_rooms_lock = client_rooms.lock().await;
        if let Some(client_room_set) = client_rooms_lock.remove(&addr) {
            let mut rooms_lock = rooms.lock().await;
            let clients_lock = clients.lock().await;
            
            for room_id in client_room_set.clone() {
                if let Some(participants) = rooms_lock.get_mut(&room_id) {
                    participants.remove(&addr);
                    
                    // Send UserLeft notification if user_id was known
                    if let Some(uid) = &user_id {
                        for participant_addr in participants.iter() {
                            if let Some(participant_tx) = clients_lock.get(participant_addr) {
                                let system_message = crate::models::ChatMessage {
                                    role: crate::models::Role::System,
                                    content: format!("User {} left the chat", uid),
                                    name: Some("system".to_string()),
                                };
                                let room_message = RoomMessage {
                                    room_id: room_id.clone(),
                                    payload: system_message,
                                };
                                let _ = participant_tx.send(room_message);
                            }
                        }
                    }
                    
                    if participants.is_empty() {
                        rooms_lock.remove(&room_id);
                    }
                }
            }
        }
    }
}