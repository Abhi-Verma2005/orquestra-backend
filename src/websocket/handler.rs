// src/websocket/handler.rs
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{accept_hdr_async, tungstenite::{Message, handshake::server::Request}};
use futures_util::{SinkExt, StreamExt};
use crate::orchestrator::service::OrchestratorService;
use crate::websocket::protocol::{
    Clients, ClientRooms, Rooms, RoomMessage, Tx, WebSocketMessage, JoinRoomMessage, SendMessageData, UserClients, WalletClients, WalletAddresses
};
use crate::websocket::MessageType;

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
                    println!("⚠️  Failed to write HTTP response to {}: {:?}", addr, e);
                }
                let _ = stream.shutdown().await;
                println!("✅ Responded to HTTP health check from {}", addr);
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
    tx: Tx,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    orchestrator: Arc<OrchestratorService>,
) {
    println!("Client connected: {}", addr);
    
    // Try to accept as WebSocket first - if it fails with WrongHttpMethod, handle as HTTP
    let ws_stream = match accept_hdr_async(stream, |req: &Request, response| {
        // Check if it's actually a WebSocket upgrade request
        let is_websocket = req.headers().get("upgrade")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);
        
        if !is_websocket {
            println!("ℹ️  HTTP request (not WebSocket) from {}: {} {}", addr, req.method(), req.uri());
        } else {
            println!("📥 WebSocket upgrade request from {}: {} {}", addr, req.method(), req.uri());
        }
        
        Ok(response)
    }).await {
        Ok(ws) => ws,
        Err(e) => {
            // Check if it's an HTTP request (not a WebSocket upgrade)
            let error_str = e.to_string();
            if error_str.contains("WrongHttpMethod") || error_str.contains("HTTP method") {
                println!("ℹ️  HTTP request received from {} (not WebSocket upgrade)", addr);
                // We can't handle HTTP here since the stream was consumed
                // The HTTP server on PORT should handle these
                return;
            }
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
            // Check if this is a direct WebSocket message (for streaming events)
            if msg.room_id == "__websocket_message__" && msg.payload.name == Some("__ws_direct__".to_string()) {
                // Send the WebSocketMessage JSON directly (content contains the WebSocketMessage JSON)
                let json_str = msg.payload.content;
                println!("📤 [{}] Sending direct WebSocket message: {}", addr_for_tx, json_str.chars().take(100).collect::<String>());
                if ws_sender_clone.send(Message::Text(json_str)).await.is_err() {
                    println!("❌ [{}] Failed to send direct WebSocket message, closing connection", addr_for_tx);
                    break;
                } else {
                    println!("✅ [{}] Direct WebSocket message sent successfully", addr_for_tx);
                }
            } else {
                // Regular RoomMessage - serialize as JSON
                if let Ok(json_str) = serde_json::to_string(&msg) {
                    println!("📤 [{}] Broadcasting RoomMessage to client: {}", addr_for_tx, json_str.chars().take(100).collect::<String>());
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
        }
    });
    
    // Handle incoming messages in the current task
    let addr_clone2 = addr.clone();
    let clients_clone = clients.clone();
    let rooms_clone = rooms.clone();
    let client_rooms_clone = client_rooms.clone();
    let user_clients_clone = user_clients.clone();
    let wallet_clients_clone = wallet_clients.clone();
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
                            wallet_clients_clone.clone(),
                            orchestrator_clone.clone(),
                        ).await;
                    }
                    Err(e) => {
                        println! ("❌ [{}] Failed to parse WebSocket message: {} | Raw text: {}", addr_clone2, e, text);
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
    cleanup_client(addr, clients, rooms, client_rooms, user_clients, wallet_clients).await;
    println!("Client disconnected: {}", addr);
}

async fn handle_websocket_message(
    message: WebSocketMessage,
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
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
                    
                    // Store latest wallet addresses for this connection, if provided
                    if let Some(wallet_addrs) = send_data.wallet_addresses.clone() {
                        println!("💼 [{}] Wallet addresses received in payload: solana={:?}, ethereum={:?}", 
                            addr, wallet_addrs.solana, wallet_addrs.ethereum);
                        let mut wallet_clients_lock = wallet_clients.lock().await;
                        wallet_clients_lock.insert(addr, wallet_addrs);
                    } else {
                        println!("⚠️ [{}] No wallet addresses in message payload", addr);
                    }
                    
                    handle_chat_message(addr, send_data, clients.clone(), rooms.clone(), user_clients.clone(), wallet_clients.clone(), orchestrator).await;
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

async fn send_websocket_event_to_client(
    addr: SocketAddr,
    message_type: MessageType,
    payload: serde_json::Value,
    clients: &Clients,
) {
    let clients_lock = clients.lock().await;
    if let Some(tx) = clients_lock.get(&addr) {
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
        // Frontend will check if content is a JSON WebSocketMessage and handle accordingly
        let room_msg = RoomMessage {
            room_id: "__websocket_message__".to_string(), // Special marker for direct WebSocket messages
            payload: crate::models::ChatMessage {
                role: crate::models::Role::System,
                content: json_str,
                name: Some("__ws_direct__".to_string()), // Marker to indicate this is a direct WebSocket message
            },
        };
        let _ = tx.send(room_msg);
    }
}

/// Stream a block of assistant text to all participants in a room using
/// TextStream / TextStreamEnd events understood by the frontend.
async fn stream_text_to_room(
    room_id: &str,
    text: &str,
    clients: &Clients,
    rooms: &Rooms,
) {
    // Get a snapshot of participants in this room
    let participant_addrs: Vec<SocketAddr> = {
        let rooms_lock = rooms.lock().await;
        if let Some(set) = rooms_lock.get(room_id) {
            set.iter().cloned().collect()
        } else {
            return;
        }
    };

    if participant_addrs.is_empty() || text.is_empty() {
        return;
    }

    // Word-based chunking (5 words at a time, matching TypeScript implementation)
    let words: Vec<&str> = text.split_whitespace().collect();
    let chunk_size = 5;
    let total_words = words.len();

    for i in (0..total_words).step_by(chunk_size) {
        let end = (i + chunk_size).min(total_words);
        let chunk_words = &words[i..end];
        let chunk = chunk_words.join(" ") + if end < total_words { " " } else { "" };

        for addr in &participant_addrs {
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

    // Send final TextStreamEnd event with full text for each participant
    for addr in &participant_addrs {
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
        ai_message.content = final_content.clone();
    
        // Phase 1: Intent Detection
        println!("🔍 [{}] Phase 1: Detecting wallet intent...", addr);
        match orchestrator.detect_wallet_intent(&final_content).await {
            Ok(needs_wallet_tool) => {
                if needs_wallet_tool {
                    println!("✅ [{}] Wallet intent detected - checking for connected wallets", addr);
                    
                    // Check for connected wallet addresses - prioritize current message payload, fallback to stored
                    let wallet_addresses = if let Some(addrs) = chat_data.wallet_addresses.clone() {
                        println!("💼 [{}] Using wallet addresses from current message payload: solana={:?}, ethereum={:?}", 
                            addr, addrs.solana, addrs.ethereum);
                        // Use addresses from current message payload (most reliable, especially for first message)
                        Some(addrs)
                    } else {
                        // Fallback to stored wallet addresses if not in current message
                        let wallet_clients_lock = wallet_clients.lock().await;
                        let stored = wallet_clients_lock.get(&addr).cloned();
                        if let Some(ref addrs) = stored {
                            println!("💼 [{}] Using stored wallet addresses: solana={:?}, ethereum={:?}", 
                                addr, addrs.solana, addrs.ethereum);
                        } else {
                            println!("⚠️ [{}] No wallet addresses found in payload or stored", addr);
                        }
                        stored
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
                            println!("💰 [{}] Fetching Solana portfolio for: {}", addr, solana_addr);
                            match crate::tools::wallet::fetch_solana_balance(
                                orchestrator.get_solana_client(),
                                solana_addr,
                            ).await {
                                Ok(portfolio) => {
                                    println!("✅ [{}] Successfully fetched Solana portfolio", addr);
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
                                    eprintln!("❌ [{}] Failed to fetch Solana portfolio: {}", addr, e);
                                    tool_results.push_str(&format!("Solana: Error - {}\n", e));
                                }
                            }
                        }
                        
                        // Fetch Ethereum portfolio if address exists
                        if let Some(ethereum_addr) = &addresses.ethereum {
                            println!("💰 [{}] Fetching Ethereum portfolio for: {}", addr, ethereum_addr);
                            match crate::tools::wallet::fetch_ethereum_balance(
                                orchestrator.get_ethereum_client(),
                                ethereum_addr,
                            ).await {
                                Ok(portfolio) => {
                                    println!("✅ [{}] Successfully fetched Ethereum portfolio", addr);
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
                                    eprintln!("❌ [{}] Failed to fetch Ethereum portfolio: {}", addr, e);
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
                            println!("💬 [{}] Phase 2: Generating acknowledgment...", addr);
                            match orchestrator.generate_acknowledgment(&final_content, &tool_results).await {
                                Ok(ack_message) => {
                                    println!("✅ [{}] Generated acknowledgment: {}", addr, ack_message);
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &ack_message,
                                        &clients,
                                        &rooms,
                                    ).await;
                                }
                                Err(e) => {
                                    eprintln!("❌ [{}] Failed to generate acknowledgment: {}", addr, e);
                                    let fallback = "I've retrieved your wallet balance information. Check the sidebar for details.".to_string();
                                    stream_text_to_room(
                                        &chat_data.chat_id,
                                        &fallback,
                                        &clients,
                                        &rooms,
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
                            ).await;
                        }
                    } else {
                        // No wallet addresses found
                        println!("⚠️ [{}] Wallet intent detected but no wallet addresses found", addr);
                        let no_wallet_response = "I can help you check your wallet balances, but I don't see any connected wallets. Please connect your wallet first.".to_string();
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &no_wallet_response,
                            &clients,
                            &rooms,
                        ).await;
                    }
                } else {
                    // No wallet intent - normal flow
                    println!("📝 [{}] No wallet intent detected - using normal AI flow with streaming", addr);
                    match orchestrator.process_chat_message(&ai_message).await {
                        Ok(ai_response) => {
                            println!("✅ [{}] Orchestrator returned response: {}", addr, ai_response.content);
                            stream_text_to_room(
                                &chat_data.chat_id,
                                &ai_response.content,
                                &clients,
                                &rooms,
                            ).await;
                        }
                        Err(e) => {
                            eprintln!("❌ [{}] Error processing chat message: {}", addr, e);
                            send_error_message(addr, &chat_data.chat_id, &e, clients, rooms).await;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ [{}] Intent detection failed: {}", addr, e);
                // Fallback to normal flow on intent detection failure (still using streaming)
                match orchestrator.process_chat_message(&ai_message).await {
                    Ok(ai_response) => {
                        stream_text_to_room(
                            &chat_data.chat_id,
                            &ai_response.content,
                            &clients,
                            &rooms,
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