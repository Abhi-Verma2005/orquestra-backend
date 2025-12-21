// src/websocket/handler.rs
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{accept_hdr_async, tungstenite::{Message, handshake::server::Request}};
use futures_util::{SinkExt, StreamExt};
use crate::models::{ChatMessage, Role};
use crate::orchestrator::service::OrchestratorService;
use crate::utils::gen_id::generate_id;
use crate::websocket::protocol::{
    Clients, ClientRooms, Rooms, RoomMessage, Tx, ClientTx, WebSocketMessage, JoinRoomMessage, SendMessageData, UserClients, WalletClients, RoomUserIds, UserNames
};
use crate::websocket::MessageType;
use std::collections::{HashSet, HashMap};

async fn handle_http_request(mut stream: TcpStream, addr: SocketAddr) {
    // Read first bytes to get the request
    let mut buffer = [0u8; 1024];
    match stream.read(&mut buffer).await {
        Ok(n) if n > 0 => {
            let request = String::from_utf8_lossy(&buffer[..n.min(100)]);
            if request.starts_with("GET") || request.starts_with("POST") || request.starts_with("HEAD") {
                // It's an HTTP request, respond with a simple health check
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 25\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}\r\n";
                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    eprintln!("Failed to write HTTP response to {}: {:?}", addr, e);
                }
                let _ = stream.shutdown().await;
                return;
            }
        }
        _ => {}
    }
    
    // If we can't read or it's not HTTP, close the connection
    let _ = stream.shutdown().await;
}

pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    _tx: Tx, // Not used anymore (kept for backward compatibility with server.rs)
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
    orchestrator: Arc<OrchestratorService>,
) {
    // Try to accept as WebSocket first - if it fails with WrongHttpMethod, handle as HTTP
    let ws_stream = match accept_hdr_async(stream, |_req: &Request, response| {
        Ok(response)
    }).await {
        Ok(ws) => ws,
        Err(e) => {
            // Check if it's an HTTP request (not a WebSocket upgrade)
            let error_str = e.to_string();
            if error_str.contains("WrongHttpMethod") || error_str.contains("HTTP method") {
                // We can't handle HTTP here since the stream was consumed
                // The HTTP server on PORT should handle these
                return;
            }
            eprintln!("Failed WebSocket handshake from {}: {:?}", addr, e);
            return;
        }
    };
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // Create per-client mpsc channel (not broadcast - direct messaging)
    let (client_tx, mut client_rx) = mpsc::channel::<RoomMessage>(100);
    
    // Register client with its own mpsc sender
    {
        let mut clients_lock = clients.lock().await;
        let total_before = clients_lock.len();
        clients_lock.insert(addr, client_tx.clone());
        let total_after = clients_lock.len();
        eprintln!("[CONNECTION] New client connected: socket={}, total_clients={} (was {})", addr, total_after, total_before);
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
    if let Err(e) = ws_sender.send(Message::Text(welcome_json)).await {
        eprintln!("[{}] Failed to send welcome message: {:?}", addr, e);
    }
    
    // Handle outgoing messages in a separate task - receives from per-client mpsc channel
    let mut ws_sender_clone = ws_sender;
    let addr_for_tx = addr.clone();
    let client_rooms_for_task = client_rooms.clone();
    let tx_task = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            // With mpsc channels, messages are already filtered - only this client receives them!
            // Check if this is a direct WebSocket message (for streaming events)
            if msg.room_id == "__websocket_message__" && msg.payload.name == Some("__ws_direct__".to_string()) {
                // Send the WebSocketMessage JSON directly (content contains the WebSocketMessage JSON)
                let json_str = msg.payload.content;
                // eprintln!("[RECEIVER {}] Forwarding direct event to client", addr_for_tx);
                if ws_sender_clone.send(Message::Text(json_str)).await.is_err() {
                    break;
                }
            } else {
                // Regular RoomMessage - check if client is in room (safety check)
                let should_forward = {
                    let client_rooms_lock = client_rooms_for_task.lock().await;
                    if let Some(client_room_set) = client_rooms_lock.get(&addr_for_tx) {
                        client_room_set.contains(&msg.room_id)
                    } else {
                        false
                    }
                };
                
                if should_forward {
                    // Regular RoomMessage - serialize as JSON
                    if let Ok(json_str) = serde_json::to_string(&msg) {
                        // eprintln!("[RECEIVER {}] Forwarding message to client for room_id={}", addr_for_tx, msg.room_id);
                        if ws_sender_clone.send(Message::Text(json_str)).await.is_err() {
                            break;
                        }
                    } else {
                        // eprintln!("[{}] Failed to serialize message", addr_for_tx);
                    }
                } else {
                    // eprintln!("[RECEIVER {}] NOT forwarding message - client not in room_id={}", addr_for_tx, msg.room_id);
                }
            }
        }
    });
    
    // Handle incoming messages in the current task
    let addr_clone2 = addr.clone();
    let clients_clone = clients.clone();
    let rooms_clone = rooms.clone();
    let client_rooms_clone = client_rooms.clone();
    let user_clients_clone = user_clients.clone();
    let wallet_clients_clone = wallet_clients.clone();
    let room_user_ids_clone = room_user_ids.clone();
    let user_names_clone = user_names.clone();
    let orchestrator_clone = orchestrator.clone();
    
    // Process incoming messages (not spawned - handles Send issue)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<WebSocketMessage>(&text) {
                    Ok(parsed) => {
                        handle_websocket_message(
                            parsed,
                            addr_clone2,
                            clients_clone.clone(),
                            rooms_clone.clone(),
                            client_rooms_clone.clone(),
                            user_clients_clone.clone(),
                            wallet_clients_clone.clone(),
                            room_user_ids_clone.clone(),
                            user_names_clone.clone(),
                            orchestrator_clone.clone(),
                        ).await;
                    }
                    Err(e) => {
                        eprintln!("[{}] Failed to parse WebSocket message: {}", addr_clone2, e);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Ok(Message::Ping(_)) => {
                // WebSocket will automatically respond with pong
            }
            Err(e) => {
                eprintln!("[{}] WebSocket error: {:?}", addr_clone2, e);
                break;
            }
            _ => {}
        }
    }
    
    // Cancel the outgoing task when we're done
    tx_task.abort();
    
    // Cleanup on disconnect
    cleanup_client(addr, clients, rooms, client_rooms, user_clients, wallet_clients).await;
}

async fn handle_websocket_message(
    message: WebSocketMessage,
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
    orchestrator: Arc<OrchestratorService>,
) {
    match message.message_type {
        MessageType::JoinChat => {
            match serde_json::from_value::<JoinRoomMessage>(message.payload) {
                Ok(join_data) => {
                    // Store user_id mapping if provided
                    if let Some(user_id) = join_data.user_id.clone() {
                        let mut user_clients_lock = user_clients.lock().await;
                        user_clients_lock.insert(addr, user_id.clone());
                        
                        // Store user_name mapping if provided
                        if let Some(user_name) = join_data.user_name.clone() {
                            let mut user_names_lock = user_names.lock().await;
                            user_names_lock.insert(user_id, user_name);
                        }
                    }
                    
                    join_room(addr, join_data.chat_id.clone(), join_data.user_name.clone(), rooms.clone(), client_rooms.clone(), user_clients.clone(), clients.clone(), room_user_ids.clone(), user_names.clone()).await;
                }
                Err(e) => {
                    eprintln!("[{}] Failed to parse JoinChat payload: {}", addr, e);
                }
            }
        }
        MessageType::ChatMessage => {
            let payload_clone = message.payload.clone();
            match serde_json::from_value::<SendMessageData>(message.payload) {
                Ok(send_data) => {
                    // Store user_id if provided in message
                    if let Some(user_id) = send_data.user_id.clone() {
                        let mut user_clients_lock = user_clients.lock().await;
                        user_clients_lock.insert(addr, user_id);
                    }
                    
                    // Store latest wallet addresses for this connection, if provided
                    if let Some(wallet_addrs) = send_data.wallet_addresses.clone() {
                        let mut wallet_clients_lock = wallet_clients.lock().await;
                        wallet_clients_lock.insert(addr, wallet_addrs);
                    }
                    
                    handle_chat_message(addr, send_data, clients.clone(), rooms.clone(), user_clients.clone(), wallet_clients.clone(), room_user_ids.clone(), user_names.clone(), orchestrator).await;
                }
                Err(e) => {
                    eprintln!("[{}] Failed to parse ChatMessage payload: {}", addr, e);
                }
            }
        }
        MessageType::LeaveChat => {
            // LeaveChat not implemented yet
        }
        _ => {
            eprintln!("[{}] Unknown message type: {:?}", addr, message.message_type);
        }
    }
}

async fn join_room(
    addr: SocketAddr,
    room_id: String,
    user_name: Option<String>,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    clients: Clients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
) {
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    // eprintln!("[JOIN_ROOM] Socket {} joining room_id={}, user_id={:?}", addr, room_id, user_id);
    
    {
        let mut rooms_lock = rooms.lock().await;
        let entry = rooms_lock.entry(room_id.clone()).or_default();
        let is_new_addr = !entry.contains(&addr);
        let room_size_before = entry.len();
        entry.insert(addr);
        let room_size_after = entry.len();
        
        // eprintln!("[JOIN_ROOM] Room {} now has {} participants (was {})", room_id, room_size_after, room_size_before);
        
        // Track user_ids per room to prevent duplicate notifications
        let mut room_user_ids_lock = room_user_ids.lock().await;
        let room_user_set = room_user_ids_lock.entry(room_id.clone()).or_default();
        
        // Check if this is a new user_id joining (not just a new connection from same user)
        let is_new_user = if let Some(ref uid) = user_id {
            !room_user_set.contains(uid)
        } else {
            false
        };
        
        // Only send UserJoined notification if:
        // 1. This is a new user_id (not just a new connection from same user)
        // 2. User ID is available
        // 3. There are multiple unique users (it's actually a group chat)
        if is_new_user && user_id.is_some() && room_user_set.len() > 0 {
            let user_id_str = user_id.as_ref().unwrap().clone();
            let display_name = user_name.clone().unwrap_or_else(|| user_id_str.clone());
            
            // Store user_name mapping
            if let Some(ref name) = user_name {
                let mut user_names_lock = user_names.lock().await;
                user_names_lock.insert(user_id_str.clone(), name.clone());
            }
            
            // Add this user_id to the room's user set BEFORE sending notifications
            // This prevents duplicate notifications if the same user joins multiple times quickly
            room_user_set.insert(user_id_str.clone());
            
            let clients_lock = clients.lock().await;
            let user_clients_lock = user_clients.lock().await;
            
            // Collect all unique user_ids in the room (excluding the new joiner)
            let unique_user_ids: Vec<String> = room_user_set.iter()
                .filter(|uid| *uid != &user_id_str)
                .cloned()
                .collect();
            
            // Send notification to each unique user ONCE (only to their first connection)
            // Use a set to track which users we've already notified to prevent duplicates
            let mut notified_users = HashSet::new();
            for target_user_id in unique_user_ids {
                if notified_users.contains(&target_user_id) {
                    continue; // Already notified this user
                }
                notified_users.insert(target_user_id.clone());
                
                // Find the first connection for this user
                if let Some(target_addr) = entry.iter().find(|addr| {
                    user_clients_lock.get(addr).map(|uid| uid == &target_user_id).unwrap_or(false)
                }) {
                    if let Some(participant_tx) = clients_lock.get(target_addr) {
                        let system_message = crate::models::ChatMessage {
                            id: Some(generate_id(Role::System)),
                            role: crate::models::Role::System,
                            content: format!("{} joined the chat", display_name),
                            name: Some("system".to_string()),
                        };
                        let room_message = RoomMessage {
                            room_id: room_id.clone(),
                            payload: system_message,
                        };
                        // Note: We can't await here because we're holding locks, so spawn a task
                        let participant_tx_clone = participant_tx.clone();
                        tokio::spawn(async move {
                            let _ = participant_tx_clone.send(room_message).await;
                        });
                        // Continue to next user (don't break - we want to notify all users)
                    }
                }
            }
        } else if is_new_addr && user_id.is_some() {
            // Still track the user_id even if we don't send notification
            if let Some(ref uid) = user_id {
                room_user_set.insert(uid.clone());
                // Store user_name if provided
                if let Some(ref name) = user_name {
                    let mut user_names_lock = user_names.lock().await;
                    user_names_lock.insert(uid.clone(), name.clone());
                }
            }
        }
    }

    {
        let mut client_rooms_lock = client_rooms.lock().await;
        let entry = client_rooms_lock.entry(addr).or_default();
        let was_in_room = entry.contains(&room_id);
        entry.insert(room_id.clone());
        let client_rooms_list: Vec<String> = entry.iter().cloned().collect();
        // eprintln!("[JOIN_ROOM] Socket {} client_rooms updated: {:?} (was already in room: {})", 
            // addr, client_rooms_list, was_in_room);
    }
}

async fn send_websocket_event_to_client(
    addr: SocketAddr,
    message_type: MessageType,
    payload: serde_json::Value,
    clients: &Clients,
) {
    // eprintln!("[SEND_EVENT] Sending event type={:?} to socket={}", message_type, addr);
    let clients_lock = clients.lock().await;
    if let Some(client_tx) = clients_lock.get(&addr) {
        let ws_message = WebSocketMessage {
            message_type,
            payload,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            message_id: format!("msg_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()),
        };
        let json_str = serde_json::to_string(&ws_message).unwrap();
        // Send WebSocketMessage directly as a special RoomMessage that frontend can detect
        // With mpsc channels, this goes directly to the target client - no filtering needed!
        let room_msg = RoomMessage {
            room_id: "__websocket_message__".to_string(), // Special marker for direct WebSocket messages
            payload: crate::models::ChatMessage {
                id: Some(generate_id(Role::System)),
                role: crate::models::Role::System,
                content: json_str,
                name: Some("__ws_direct__".to_string()), // Marker to indicate this is a direct WebSocket message
            },
        };
        // eprintln!("[SEND_EVENT] Sending directly to client's mpsc channel");
        let send_result = client_tx.send(room_msg).await;
        // if send_result.is_err() {
        //     // eprintln!("[SEND_EVENT] ERROR sending event to socket={}: {:?}", addr, send_result.err());
        // } else {
        //     // eprintln!("[SEND_EVENT] Successfully sent event to socket={}", addr);
        // }
    } else {
        // eprintln!("[SEND_EVENT] WARNING: No client_tx found for socket={}", addr);
    }
}

/// Stream a block of assistant text to all participants in a room using
/// TextStream / TextStreamEnd events understood by the frontend.
/// CRITICAL: Only sends to one connection per unique user_id to prevent duplicates.
async fn stream_text_to_room(
    room_id: &str,
    text: &str,
    clients: &Clients,
    rooms: &Rooms,
    user_clients: &UserClients,
) {
    // Get a snapshot of participants in this room
    let (participant_addrs, user_clients_snapshot): (Vec<SocketAddr>, HashMap<SocketAddr, String>) = {
        let rooms_lock = rooms.lock().await;
        let user_clients_lock = user_clients.lock().await;
        if let Some(set) = rooms_lock.get(room_id) {
            let addrs: Vec<SocketAddr> = set.iter().cloned().collect();
            
            let mut user_map: HashMap<SocketAddr, String> = HashMap::new();
            for addr in &addrs {
                if let Some(uid) = user_clients_lock.get(addr) {
                    user_map.insert(*addr, uid.clone());
                }
            }
            (addrs, user_map)
        } else {
            return;
        }
    };

    if participant_addrs.is_empty() || text.is_empty() {
        return;
    }

    // Track which users we've sent to (to avoid duplicates from multiple connections)
    let mut sent_to_users = HashSet::new();
    
    // Get unique addresses to send to (one per user_id)
    let mut addresses_to_send: Vec<SocketAddr> = Vec::new();
    for addr in &participant_addrs {
        if let Some(user_id) = user_clients_snapshot.get(addr) {
            // Only send once per user_id
            if !sent_to_users.contains(user_id) {
                sent_to_users.insert(user_id.clone());
                addresses_to_send.push(*addr);
            }
        } else {
            // No user_id - send to this connection anyway (fallback)
            addresses_to_send.push(*addr);
        }
    }

    // Word-based chunking (5 words at a time, matching TypeScript implementation)
    let words: Vec<&str> = text.split_whitespace().collect();
    let chunk_size = 5;
    let total_words = words.len();

    for i in (0..total_words).step_by(chunk_size) {
        let end = (i + chunk_size).min(total_words);
        let chunk_words = &words[i..end];
        let chunk = chunk_words.join(" ") + if end < total_words { " " } else { "" };

        // Only send to unique users (one connection per user_id)
        for addr in &addresses_to_send {
            send_websocket_event_to_client(
                *addr,
                MessageType::TextStream,
                serde_json::json!({ 
                    "text": chunk,
                    "isComplete": end >= total_words
                }),
                clients,
            )
            .await;
        }

        // 50ms delay between chunks (matching TypeScript implementation)
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Send final TextStreamEnd event with full text for each unique user
    for addr in &addresses_to_send {
        send_websocket_event_to_client(
            *addr,
            MessageType::TextStreamEnd,
            serde_json::json!({ "text": text }),
            clients,
        )
        .await;
    }
}

async fn handle_chat_message(
    addr: SocketAddr,
    chat_data: SendMessageData,
    clients: Clients,
    rooms: Rooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
    orchestrator: Arc<OrchestratorService>,
) {
    // Get sender info
    let sender_user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    // Get total connected clients count
    let total_connected_clients = {
        let clients_lock = clients.lock().await;
        clients_lock.len()
    };
    
    // eprintln!("[HANDLE_MSG] Received message from sender={}, user_id={:?}, room_id={}, total_connected_clients={}", 
        // addr, sender_user_id, chat_data.chat_id, total_connected_clients);
    
    // STEP 1: Get room status and participants
    let (room_exists, _participant_details, unique_users_in_room) = {
        let rooms_lock = rooms.lock().await;
        let user_clients_lock = user_clients.lock().await;
        let room_user_ids_lock = room_user_ids.lock().await;
        
        if let Some(participants) = rooms_lock.get(&chat_data.chat_id) {
            // Build detailed participant info
            let mut details: Vec<(SocketAddr, Option<String>)> = Vec::new();
            for participant_addr in participants.iter() {
                let user_id = user_clients_lock.get(participant_addr).cloned();
                details.push((*participant_addr, user_id));
            }
            
            // Get unique user_ids in this room
            let unique_users: HashSet<String> = participants.iter()
                .filter_map(|addr| user_clients_lock.get(addr).cloned())
                .collect();
            
            // Also check room_user_ids for additional safety
            let room_user_set = room_user_ids_lock.get(&chat_data.chat_id)
                .map(|set| set.clone())
                .unwrap_or_default();
            
            let total_unique_users = unique_users.union(&room_user_set).count();
            
            // eprintln!("[HANDLE_MSG] Room {} exists with {} participants (sockets), {} unique users", 
                // chat_data.chat_id, participants.len(), total_unique_users);
            // eprintln!("[HANDLE_MSG] Participants: {:?}", details);
            
            (true, details, total_unique_users)
        } else {
            // eprintln!("[HANDLE_MSG] Room {} does NOT exist!", chat_data.chat_id);
            (false, Vec::new(), 0)
        }
    };
    
    if !room_exists {
        // eprintln!("[HANDLE_MSG] Exiting early - room does not exist");
        return;
    }
    
    // STEP 2: Get user_id for sender
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    // STEP 3: Check if this is a group chat
    let is_group_chat = chat_data.is_group_chat.unwrap_or(false);
    
    // CRITICAL VALIDATION: For individual chats (not group chats), ensure only one user is in the room
    if !is_group_chat && unique_users_in_room > 1 {
        let error_msg = format!("Error: This chat room has multiple users ({}), which is not allowed for individual chats. Please create a new chat.", unique_users_in_room);
        send_error_message(addr, &chat_data.chat_id, &error_msg, clients.clone(), rooms.clone()).await;
        return;
    }
    
    // Get user_name for this user_id
    let user_name = if let Some(ref uid) = user_id {
        let user_names_lock = user_names.lock().await;
        user_names_lock.get(uid).cloned()
    } else {
        None
    };
    
    let content = chat_data.message.payload.content.trim();
    
    // Message routing logic:
    // - If NOT group chat: route all messages to AI (existing behavior)
    // - If IS group chat: check for "@ai" prefix
    let has_ai_prefix = content.to_lowercase().starts_with("@ai");
    let should_route_to_ai = !is_group_chat || has_ai_prefix;

    println!("Reached here {}", should_route_to_ai);
    
    if should_route_to_ai {
        // For group chats, broadcast the user's message first so everyone sees what was asked
        if is_group_chat {
            // Use the original message's name field if present (for deduplication), otherwise use user_id
            let sender_id = chat_data.message.payload.name.clone()
                .or_else(|| user_id.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let user_message = crate::models::ChatMessage {
                id: Some(generate_id(Role::User)),
                role: crate::models::Role::User,
                content: content.to_string(), // Original message with @ai prefix
                name: Some(sender_id), // Preserve original name for frontend deduplication
            };
            broadcast_to_room(
                &chat_data.chat_id,
                user_message,
                clients.clone(),
                rooms.clone(),
                user_clients.clone(),
                room_user_ids.clone(),
                user_names.clone(),
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
        ai_message.content = final_content.clone();
    
        // Phase 1: Intent Detection
        match orchestrator.detect_wallet_intent(&final_content).await {
            Ok(needs_wallet_tool) => {
                if needs_wallet_tool {
                    // Check for connected wallet addresses - prioritize current message payload, fallback to stored
                    let wallet_addresses = if let Some(addrs) = chat_data.wallet_addresses.clone() {
                        // Use addresses from current message payload (most reliable, especially for first message)
                        Some(addrs)
                    } else {
                        // Fallback to stored wallet addresses if not in current message
                        let wallet_clients_lock = wallet_clients.lock().await;
                        wallet_clients_lock.get(&addr).cloned()
                    };
                    
                    if let Some(addresses) = wallet_addresses {
                        // STEP 1: Send FunctionCall message IMMEDIATELY (before execution)
                        // This shows the tool call UI right away
                        let function_name = "fetchWalletBalance";
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCall,
                            serde_json::json!({
                                "name": function_name,
                                "args": {
                                    "solana": addresses.solana.clone(),
                                    "ethereum": addresses.ethereum.clone()
                                },
                                "role": "function"
                            }),
                            &clients,
                        ).await;
                        
                        // STEP 2: Stream any reasoning text FIRST (if we had any from AI)
                        // For now, we'll skip this since intent detection is separate
                        // In future, this will come from AI response with function calling
                        
                        // STEP 3: Send FunctionCallStart - tool execution starting
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCallStart,
                            serde_json::json!({
                                "name": function_name
                            }),
                            &clients,
                        ).await;
                        
                        let mut portfolios = Vec::new();
                        let mut tool_results = String::new();
                        
                        // STEP 4: Execute function
                        // Fetch Solana portfolio if address exists
                        if let Some(solana_addr) = &addresses.solana {
                            match crate::tools::wallet::fetch_solana_balance(
                                orchestrator.get_solana_client(),
                                solana_addr,
                            ).await {
                                Ok(portfolio) => {
                                    let portfolio_json = serde_json::to_value(&portfolio).unwrap_or_default();
                                    portfolios.push(("solana".to_string(), portfolio_json.clone()));
                                    tool_results.push_str(&format!("Solana: {} SOL worth ${:.2}\n", 
                                        portfolio.native_balance, portfolio.native_value_usd));
                                    
                                    // Send wallet data event
                                    send_websocket_event_to_client(
                                        addr,
                                        MessageType::WalletData,
                                        serde_json::json!({
                                            "chain": "solana",
                                            "data": portfolio_json
                                        }),
                                        &clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to fetch Solana portfolio: {}", addr, e);
                                    tool_results.push_str(&format!("Solana: Error - {}\n", e));
                                }
                            }
                        }
                        
                        // Fetch Ethereum portfolio if address exists
                        if let Some(ethereum_addr) = &addresses.ethereum {
                            match crate::tools::wallet::fetch_ethereum_balance(
                                orchestrator.get_ethereum_client(),
                                ethereum_addr,
                            ).await {
                                Ok(portfolio) => {
                                    let portfolio_json = serde_json::to_value(&portfolio).unwrap_or_default();
                                    portfolios.push(("ethereum".to_string(), portfolio_json.clone()));
                                    tool_results.push_str(&format!("Ethereum: {} ETH worth ${:.2}\n", 
                                        portfolio.native_balance, portfolio.native_value_usd));
                                    
                                    // Send wallet data event
                                    send_websocket_event_to_client(
                                        addr,
                                        MessageType::WalletData,
                                        serde_json::json!({
                                            "chain": "ethereum",
                                            "data": portfolio_json
                                        }),
                                        &clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to fetch Ethereum portfolio: {}", addr, e);
                                    tool_results.push_str(&format!("Ethereum: Error - {}\n", e));
                                }
                            }
                        }
                        
                        // STEP 5: Send FunctionCallEnd - tool execution finished
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCallEnd,
                            serde_json::json!({
                                "name": function_name
                            }),
                            &clients,
                        ).await;
                        
                        // STEP 6: Send FunctionResult with tool results
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionResult,
                            serde_json::json!({
                                "name": function_name,
                                "result": {
                                    "portfolios": portfolios,
                                    "summary": tool_results
                                },
                                "role": "function"
                            }),
                            &clients,
                        ).await;
                        
                        // Send open sidebar event if we have any portfolio data
                        if !portfolios.is_empty() {
                            send_websocket_event_to_client(
                                addr,
                                MessageType::OpenSidebar,
                                serde_json::json!({
                                    "message": "Opening sidebar to display wallet data"
                                }),
                                &clients,
                            ).await;
                            
                            // STEP 7: Stream final summary
                            match orchestrator.generate_acknowledgment(&final_content, &tool_results).await {
                                Ok(ack_message) => {
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &ack_message,
                                        &clients,
                                        &rooms,
                                        &user_clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to generate acknowledgment: {}", addr, e);
                                    let fallback = "I've retrieved your wallet balance information. Check the sidebar for details.".to_string();
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &fallback,
                                        &clients,
                                        &rooms,
                                        &user_clients,
                                    ).await;
                                }
                            }
                        } else {
                            // No wallets connected
                            let no_wallet_response = "I can help you check your wallet balances, but I don't see any connected wallets. Please connect your wallet first.".to_string();
                            stream_text_to_room(
                                &chat_data.chat_id,
                                &no_wallet_response,
                                &clients,
                                &rooms,
                                &user_clients,
                            ).await;
                        }
                    } else {
                        // No wallet addresses found
                        let no_wallet_response = "I can help you check your wallet balances, but I don't see any connected wallets. Please connect your wallet first.".to_string();
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &no_wallet_response,
                            &clients,
                            &rooms,
                            &user_clients,
                        ).await;
                    }
                } else {
                    // No wallet intent - normal flow
                    match orchestrator.process_chat_message(&ai_message).await {
                        Ok(ai_response) => {
                            stream_text_to_room(
                                &chat_data.chat_id,
                                &ai_response.content,
                                &clients,
                                &rooms,
                                &user_clients,
                            ).await;
                        }
                        Err(e) => {
                            eprintln!("[{}] Error processing chat message: {}", addr, e);
                            send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[{}] Intent detection failed: {}", addr, e);
                // Fallback to normal flow on intent detection failure (still using streaming)
                match orchestrator.process_chat_message(&ai_message).await {
                    Ok(ai_response) => {
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &ai_response.content,
                            &clients,
                            &rooms,
                            &user_clients,
                        ).await;
                    }
                    Err(err) => {
                        send_error_message(addr, &chat_data.chat_id, &err, clients, rooms).await;
                    }
                }
            }
        }
    } else if is_group_chat {
        // Regular group message - broadcast to all participants (only if group chat)
        // Use the original message's name field if present (for deduplication), otherwise use user_id
        let sender_id = chat_data.message.payload.name.clone()
            .or_else(|| user_id.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        
        let group_message = ChatMessage {
            id: Some(generate_id(Role::User)),
            role: Role::User,
            content: content.to_string(),
            name: Some(sender_id), // Preserve original name for frontend deduplication
        };
        
        // Broadcast to all participants including sender (so everyone sees it in same order)
        // CRITICAL: Only broadcast to users in this specific room
        broadcast_to_room(
            &chat_data.chat_id,
            group_message,
            clients.clone(),
            rooms.clone(),
            user_clients.clone(),
            room_user_ids.clone(),
            user_names.clone(),
        ).await;
        
        // Also save the message to database immediately
        // Note: Each client will also try to save, but this ensures it's saved at least once
    } else {
        // Not a group chat and no "@ai" prefix - this shouldn't happen with correct frontend logic
        // But fallback: route to AI anyway (existing behavior)
        let ai_message = chat_data.message.payload.clone();

        // [TODO] unwrap here is unsafe make it safe 
        match orchestrator.process_chat_message_orq(&ai_message).await {
            Ok(ai_response) => {
                broadcast_to_room(
                    &chat_data.chat_id,
                    ai_response,
                    clients.clone(),
                    rooms.clone(),
                    user_clients.clone(),
                    room_user_ids.clone(),
                    user_names.clone(),
                ).await;
            }
            Err(e) => {
                eprintln!("[{}] Error processing chat message: {}", addr, e);
                send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
            }
        }
    }
}

// ====================== // Main Orquestration Logic // ======================== //

async fn handle_chat_message_orq(
    addr: SocketAddr,
    chat_data: SendMessageData,
    clients: Clients,
    rooms: Rooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
    orchestrator: Arc<OrchestratorService>,
) {
    // Get sender info
    let sender_user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    // Get total connected clients count
    let total_connected_clients = {
        let clients_lock = clients.lock().await;
        clients_lock.len()
    };
    
    // eprintln!("[HANDLE_MSG] Received message from sender={}, user_id={:?}, room_id={}, total_connected_clients={}", 
        // addr, sender_user_id, chat_data.chat_id, total_connected_clients);
    
    // STEP 1: Get room status and participants
    let (room_exists, _participant_details, unique_users_in_room) = {
        let rooms_lock = rooms.lock().await;
        let user_clients_lock = user_clients.lock().await;
        let room_user_ids_lock = room_user_ids.lock().await;
        
        if let Some(participants) = rooms_lock.get(&chat_data.chat_id) {
            // Build detailed participant info
            let mut details: Vec<(SocketAddr, Option<String>)> = Vec::new();
            for participant_addr in participants.iter() {
                let user_id = user_clients_lock.get(participant_addr).cloned();
                details.push((*participant_addr, user_id));
            }
            
            // Get unique user_ids in this room
            let unique_users: HashSet<String> = participants.iter()
                .filter_map(|addr| user_clients_lock.get(addr).cloned())
                .collect();
            
            // Also check room_user_ids for additional safety
            let room_user_set = room_user_ids_lock.get(&chat_data.chat_id)
                .map(|set| set.clone())
                .unwrap_or_default();
            
            let total_unique_users = unique_users.union(&room_user_set).count();
            
            // eprintln!("[HANDLE_MSG] Room {} exists with {} participants (sockets), {} unique users", 
            //     chat_data.chat_id, participants.len(), total_unique_users);
            // eprintln!("[HANDLE_MSG] Participants: {:?}", details);
            
            (true, details, total_unique_users)
        } else {
            // eprintln!("[HANDLE_MSG] Room {} does NOT exist!", chat_data.chat_id);
            (false, Vec::new(), 0)
        }
    };
    
    if !room_exists {
        // eprintln!("[HANDLE_MSG] Exiting early - room does not exist");
        return;
    }
    
    // STEP 2: Get user_id for sender
    let user_id = {
        let user_clients_lock = user_clients.lock().await;
        user_clients_lock.get(&addr).cloned()
    };
    
    // STEP 3: Check if this is a group chat
    let is_group_chat = chat_data.is_group_chat.unwrap_or(false);
    
    // CRITICAL VALIDATION: For individual chats (not group chats), ensure only one user is in the room
    if !is_group_chat && unique_users_in_room > 1 {
        let error_msg = format!("Error: This chat room has multiple users ({}), which is not allowed for individual chats. Please create a new chat.", unique_users_in_room);
        send_error_message(addr, &chat_data.chat_id, &error_msg, clients.clone(), rooms.clone()).await;
        return;
    }
    
    // Get user_name for this user_id
    let user_name = if let Some(ref uid) = user_id {
        let user_names_lock = user_names.lock().await;
        user_names_lock.get(uid).cloned()
    } else {
        None
    };
    
    let content = chat_data.message.payload.content.trim();
    
    // Message routing logic:
    // - If NOT group chat: route all messages to AI (existing behavior)
    // - If IS group chat: check for "@ai" prefix
    let has_ai_prefix = content.to_lowercase().starts_with("@ai");
    let should_route_to_ai = !is_group_chat || has_ai_prefix;
    
    if should_route_to_ai {
        // For group chats, broadcast the user's message first so everyone sees what was asked
        if is_group_chat {
            // Use the original message's name field if present (for deduplication), otherwise use user_id
            let sender_id = chat_data.message.payload.name.clone()
                .or_else(|| user_id.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let user_message = crate::models::ChatMessage {
                id: Some(generate_id(Role::User)),
                role: crate::models::Role::User,
                content: content.to_string(), // Original message with @ai prefix
                name: Some(sender_id), // Preserve original name for frontend deduplication
            };
            broadcast_to_room(
                &chat_data.chat_id,
                user_message,
                clients.clone(),
                rooms.clone(),
                user_clients.clone(),
                room_user_ids.clone(),
                user_names.clone(),
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
        ai_message.content = final_content.clone();

        // match orchestrator.process_chat_message(&ai_message).await {
        //     Ok(response) => {
                
        //     }
        //     Err(e) => {

        //     }
        // } 
    
        // Phase 1: Intent Detection
        match orchestrator.detect_wallet_intent(&final_content).await {
            Ok(needs_wallet_tool) => {
                if needs_wallet_tool {
                    // Check for connected wallet addresses - prioritize current message payload, fallback to stored
                    let wallet_addresses = if let Some(addrs) = chat_data.wallet_addresses.clone() {
                        // Use addresses from current message payload (most reliable, especially for first message)
                        Some(addrs)
                    } else {
                        // Fallback to stored wallet addresses if not in current message
                        let wallet_clients_lock = wallet_clients.lock().await;
                        wallet_clients_lock.get(&addr).cloned()
                    };
                    
                    if let Some(addresses) = wallet_addresses {
                        // STEP 1: Send FunctionCall message IMMEDIATELY (before execution)
                        // This shows the tool call UI right away
                        let function_name = "fetchWalletBalance";
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCall,
                            serde_json::json!({
                                "name": function_name,
                                "args": {
                                    "solana": addresses.solana.clone(),
                                    "ethereum": addresses.ethereum.clone()
                                },
                                "role": "function"
                            }),
                            &clients,
                        ).await;
                        
                        // STEP 2: Stream any reasoning text FIRST (if we had any from AI)
                        // For now, we'll skip this since intent detection is separate
                        // In future, this will come from AI response with function calling
                        
                        // STEP 3: Send FunctionCallStart - tool execution starting
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCallStart,
                            serde_json::json!({
                                "name": function_name
                            }),
                            &clients,
                        ).await;
                        
                        let mut portfolios = Vec::new();
                        let mut tool_results = String::new();
                        
                        // STEP 4: Execute function
                        // Fetch Solana portfolio if address exists
                        if let Some(solana_addr) = &addresses.solana {
                            match crate::tools::wallet::fetch_solana_balance(
                                orchestrator.get_solana_client(),
                                solana_addr,
                            ).await {
                                Ok(portfolio) => {
                                    let portfolio_json = serde_json::to_value(&portfolio).unwrap_or_default();
                                    portfolios.push(("solana".to_string(), portfolio_json.clone()));
                                    tool_results.push_str(&format!("Solana: {} SOL worth ${:.2}\n", 
                                        portfolio.native_balance, portfolio.native_value_usd));
                                    
                                    // Send wallet data event
                                    send_websocket_event_to_client(
                                        addr,
                                        MessageType::WalletData,
                                        serde_json::json!({
                                            "chain": "solana",
                                            "data": portfolio_json
                                        }),
                                        &clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to fetch Solana portfolio: {}", addr, e);
                                    tool_results.push_str(&format!("Solana: Error - {}\n", e));
                                }
                            }
                        }
                        
                        // Fetch Ethereum portfolio if address exists
                        if let Some(ethereum_addr) = &addresses.ethereum {
                            match crate::tools::wallet::fetch_ethereum_balance(
                                orchestrator.get_ethereum_client(),
                                ethereum_addr,
                            ).await {
                                Ok(portfolio) => {
                                    let portfolio_json = serde_json::to_value(&portfolio).unwrap_or_default();
                                    portfolios.push(("ethereum".to_string(), portfolio_json.clone()));
                                    tool_results.push_str(&format!("Ethereum: {} ETH worth ${:.2}\n", 
                                        portfolio.native_balance, portfolio.native_value_usd));
                                    
                                    // Send wallet data event
                                    send_websocket_event_to_client(
                                        addr,
                                        MessageType::WalletData,
                                        serde_json::json!({
                                            "chain": "ethereum",
                                            "data": portfolio_json
                                        }),
                                        &clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to fetch Ethereum portfolio: {}", addr, e);
                                    tool_results.push_str(&format!("Ethereum: Error - {}\n", e));
                                }
                            }
                        }
                        
                        // STEP 5: Send FunctionCallEnd - tool execution finished
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionCallEnd,
                            serde_json::json!({
                                "name": function_name
                            }),
                            &clients,
                        ).await;
                        
                        // STEP 6: Send FunctionResult with tool results
                        send_websocket_event_to_client(
                            addr,
                            MessageType::FunctionResult,
                            serde_json::json!({
                                "name": function_name,
                                "result": {
                                    "portfolios": portfolios,
                                    "summary": tool_results
                                },
                                "role": "function"
                            }),
                            &clients,
                        ).await;
                        
                        // Send open sidebar event if we have any portfolio data
                        if !portfolios.is_empty() {
                            send_websocket_event_to_client(
                                addr,
                                MessageType::OpenSidebar,
                                serde_json::json!({
                                    "message": "Opening sidebar to display wallet data"
                                }),
                                &clients,
                            ).await;
                            
                            // STEP 7: Stream final summary
                            match orchestrator.generate_acknowledgment(&final_content, &tool_results).await {
                                Ok(ack_message) => {
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &ack_message,
                                        &clients,
                                        &rooms,
                                        &user_clients,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("[{}] Failed to generate acknowledgment: {}", addr, e);
                                    let fallback = "I've retrieved your wallet balance information. Check the sidebar for details.".to_string();
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &fallback,
                                        &clients,
                                        &rooms,
                                        &user_clients,
                                    ).await;
                                }
                            }
                        } else {
                            // No wallets connected
                            let no_wallet_response = "I can help you check your wallet balances, but I don't see any connected wallets. Please connect your wallet first.".to_string();
                            stream_text_to_room(
                                &chat_data.chat_id,
                                &no_wallet_response,
                                &clients,
                                &rooms,
                                &user_clients,
                            ).await;
                        }
                    } else {
                        // No wallet addresses found
                        let no_wallet_response = "I can help you check your wallet balances, but I don't see any connected wallets. Please connect your wallet first.".to_string();
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &no_wallet_response,
                            &clients,
                            &rooms,
                            &user_clients,
                        ).await;
                    }
                } else {
                    // No wallet intent - normal flow
                    match orchestrator.process_chat_message(&ai_message).await {
                        Ok(ai_response) => {
                            stream_text_to_room(
                                &chat_data.chat_id,
                                &ai_response.content,
                                &clients,
                                &rooms,
                                &user_clients,
                            ).await;
                        }
                        Err(e) => {
                            eprintln!("[{}] Error processing chat message: {}", addr, e);
                            send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[{}] Intent detection failed: {}", addr, e);
                // Fallback to normal flow on intent detection failure (still using streaming)
                match orchestrator.process_chat_message(&ai_message).await {
                    Ok(ai_response) => {
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &ai_response.content,
                            &clients,
                            &rooms,
                            &user_clients,
                        ).await;
                    }
                    Err(err) => {
                        send_error_message(addr, &chat_data.chat_id, &err, clients, rooms).await;
                    }
                }
            }
        }
    } else if is_group_chat {
        // Regular group message - broadcast to all participants (only if group chat)
        // Use the original message's name field if present (for deduplication), otherwise use user_id
        let sender_id = chat_data.message.payload.name.clone()
            .or_else(|| user_id.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        
        let group_message = crate::models::ChatMessage {
            id: Some(generate_id(Role::User)),
            role: crate::models::Role::User,
            content: content.to_string(),
            name: Some(sender_id), // Preserve original name for frontend deduplication
        };
        
        // Broadcast to all participants including sender (so everyone sees it in same order)
        // CRITICAL: Only broadcast to users in this specific room
        broadcast_to_room(
            &chat_data.chat_id,
            group_message,
            clients.clone(),
            rooms.clone(),
            user_clients.clone(),
            room_user_ids.clone(),
            user_names.clone(),
        ).await;
        
        // Also save the message to database immediately
        // Note: Each client will also try to save, but this ensures it's saved at least once
    } else {
        // Not a group chat and no "@ai" prefix - this shouldn't happen with correct frontend logic
        // But fallback: route to AI anyway (existing behavior)
        let ai_message = chat_data.message.payload.clone();
        match orchestrator.process_chat_message(&ai_message).await {
            Ok(ai_response) => {
                broadcast_to_room(
                    &chat_data.chat_id,
                    ai_response,
                    clients.clone(),
                    rooms.clone(),
                    user_clients.clone(),
                    room_user_ids.clone(),
                    user_names.clone(),
                ).await;
            }
            Err(e) => {
                eprintln!("[{}] Error processing chat message: {}", addr, e);
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
    user_clients: UserClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
) {
    eprintln!("[BROADCAST] Starting broadcast_to_room for room_id={}", room_id);
    
    // Get total connected clients
    let total_clients = {
        let clients_lock = clients.lock().await;
        clients_lock.len()
    };
    eprintln!("[BROADCAST] Total connected clients globally: {}", total_clients);
    
    let rooms_lock = rooms.lock().await;
    if let Some(participants) = rooms_lock.get(room_id) {
        let clients_lock = clients.lock().await;
        let user_clients_lock = user_clients.lock().await;
        let room_user_ids_lock = room_user_ids.lock().await;
        
        eprintln!("[BROADCAST] Room {} has {} participant sockets", room_id, participants.len());
        
        // Track which users we've sent to (to avoid duplicates from multiple connections)
        let mut sent_to_users = HashSet::new();
        let mut sockets_sent_to = Vec::new();
        
        // Send to one connection per unique user_id
        for participant_addr in participants {
            if let Some(user_id) = user_clients_lock.get(participant_addr) {
                    // Only send once per user_id
                    if !sent_to_users.contains(user_id) {
                        sent_to_users.insert(user_id.clone());
                        
                        if let Some(participant_tx) = clients_lock.get(participant_addr) {
                            let room_message = RoomMessage {
                                room_id: room_id.to_string(),
                                payload: message.clone(),
                            };
                            eprintln!("[BROADCAST] Sending to socket={}, user_id={}", participant_addr, user_id);
                            let send_result = participant_tx.send(room_message).await;
                            sockets_sent_to.push((*participant_addr, user_id.clone(), send_result.is_ok()));
                            if send_result.is_err() {
                                eprintln!("[BROADCAST] ERROR sending to socket={}: {:?}", participant_addr, send_result.err());
                            }
                        } else {
                            eprintln!("[BROADCAST] WARNING: No client_tx found for socket={}", participant_addr);
                        }
                    } else {
                        eprintln!("[BROADCAST] Skipping duplicate user_id={} at socket={}", user_id, participant_addr);
                    }
                } else {
                    // No user_id - send to this connection anyway (fallback)
                    if let Some(participant_tx) = clients_lock.get(participant_addr) {
                        let room_message = RoomMessage {
                            room_id: room_id.to_string(),
                            payload: message.clone(),
                        };
                        eprintln!("[BROADCAST] Sending to socket={} (no user_id)", participant_addr);
                        let send_result = participant_tx.send(room_message).await;
                        sockets_sent_to.push((*participant_addr, "NO_USER_ID".to_string(), send_result.is_ok()));
                        if send_result.is_err() {
                            eprintln!("[BROADCAST] ERROR sending to socket={}: {:?}", participant_addr, send_result.err());
                        }
                    } else {
                        eprintln!("[BROADCAST] WARNING: No client_tx found for socket={} (no user_id)", participant_addr);
                    }
                }
        }
        
        eprintln!("[BROADCAST] Completed broadcast_to_room for room_id={}, sent to {} sockets: {:?}", 
            room_id, sockets_sent_to.len(), sockets_sent_to);
    } else {
        eprintln!("[BROADCAST] ERROR: Room {} not found in rooms map!", room_id);
    }
}

async fn send_error_message(
    addr: SocketAddr,
    chat_id: &str,
    error_msg: &str,
    clients: Clients,
    _rooms: Rooms,
) {
    eprintln!("[SEND_ERROR] Sending error to socket={}, room_id={}, error={}", addr, chat_id, error_msg);
    // Create error chat message with a unique identifier to prevent duplicates
    let error_content = format!("Error: {}", error_msg);
    let error_chat_message = crate::models::ChatMessage {
        id: Some(generate_id(Role::Assistant)),
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
        let send_result = sender_tx.send(room_message).await;
        if let Err(e) = send_result {
            eprintln!("[SEND_ERROR] Failed to send error to sender {}: {:?}", addr, e);
        } else {
            eprintln!("[SEND_ERROR] Successfully sent error to socket={}", addr);
        }
    } else {
        eprintln!("[SEND_ERROR] WARNING: No client_tx found for socket={}", addr);
    }
}

async fn cleanup_client(
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
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
        let mut wallet_clients_lock = wallet_clients.lock().await;
        wallet_clients_lock.remove(&addr);
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
                                    id: Some(generate_id(Role::System)),
                                    role: crate::models::Role::System,
                                    content: format!("User {} left the chat", uid),
                                    name: Some("system".to_string()),
                                };
                                let room_message = RoomMessage {
                                    room_id: room_id.clone(),
                                    payload: system_message,
                                };
                                // Clone sender and spawn task since we're holding locks
                                let participant_tx_clone = participant_tx.clone();
                                let room_id_clone = room_id.clone();
                                tokio::spawn(async move {
                                    let _ = participant_tx_clone.send(room_message).await;
                                });
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