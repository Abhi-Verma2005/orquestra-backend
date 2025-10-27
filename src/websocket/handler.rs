// src/websocket/handler.rs
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use crate::websocket::protocol::{
    Clients, ClientRooms, Rooms, RoomMessage, Tx, WebSocketMessage, JoinRoomMessage, SendMessageData
};
use crate::websocket::MessageType;

pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    tx: Tx,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
) {
    println!("Client connected: {}", addr);
    
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(_) => {
            println!("Failed WebSocket handshake: {}", addr);
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
        message_type: MessageType::JoinAck,
        payload: json!({
            
        }),
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        message_id: format!("msg_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis())
    };
    
    let _ = ws_sender.send(Message::Text(serde_json::to_string(&welcome_msg).unwrap())).await;
    
    // Handle outgoing messages
    let tx_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if ws_sender.send(Message::Text(msg.content.into())).await.is_err() {
                break;
            }
        }
    });
    
    // Handle incoming messages
    let addr_clone2 = addr.clone();
    let clients_clone = clients.clone();
    let rooms_clone = rooms.clone();
    let client_rooms_clone = client_rooms.clone();
    let rx_task = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<WebSocketMessage>(&text) {
                        handle_websocket_message(
                            parsed,
                            addr_clone2,
                            clients_clone.clone(),
                            rooms_clone.clone(),
                            client_rooms_clone.clone(),
                        ).await;
                    }
                }
                Ok(Message::Frame(_)) => {},
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_)) => {
                    // WebSocket will automatically respond with pong
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Binary(_)) => {}
                Err(_) => break,
            }
        }
    });
    
    tokio::select! {
        _ = tx_task => {},
        _ = rx_task => {},
    }
    
    // Cleanup on disconnect
    cleanup_client(addr, clients, rooms, client_rooms).await;
    println!("Client disconnected: {}", addr);
}

async fn handle_websocket_message(
    message: WebSocketMessage,
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
) {
    match message.message_type {
        MessageType::JoinRoom => {
            if let Ok(join_data) = serde_json::from_value::<JoinRoomMessage>(message.payload) {
                join_room(addr, join_data.room_id, rooms.clone(), client_rooms.clone()).await;
            }
        }
        MessageType::SendMessage => {
            if let Ok(send_data) = serde_json::from_value::<SendMessageData>(message.payload) {
                broadcast_message(send_data, addr, clients.clone(), rooms.clone()).await;
            }
        }
        _ => {
            println!("Unknown message type: {}", message.message_type.to_string());
        }
    }
}

async fn join_room(
    addr: SocketAddr,
    room_id: String,
    rooms: Rooms,
    client_rooms: ClientRooms,
) {
    {
        let mut rooms_lock = rooms.lock().await;
        let entry = rooms_lock.entry(room_id.clone()).or_default();
        entry.insert(addr);
    }

    {
        let mut client_rooms_lock = client_rooms.lock().await;
        let entry = client_rooms_lock.entry(addr).or_default();
        entry.insert(room_id.clone());
    }
    println!("{} joined the room {}", addr, room_id);
}

async fn broadcast_message(
    send_data: SendMessageData,
    sender_addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
) {
    let rooms_lock = rooms.lock().await;
    if let Some(participants) = rooms_lock.get(&send_data.room_id) {
        let client_lock = clients.lock().await;
        
        for participant_addr in participants {
            if let Some(participant_tx) = client_lock.get(participant_addr) {
                let room_message = RoomMessage {
                    room_id: send_data.room_id.clone(),
                    content: send_data.message.clone(),
                };
                let _ = participant_tx.send(room_message);
            }
        }
        println!("Message broadcasted to {} participants in room {}", participants.len(), send_data.room_id);
    }
}

async fn cleanup_client(
    addr: SocketAddr,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
) {
    {
        let mut clients_lock = clients.lock().await;
        clients_lock.remove(&addr);
    }

    {
        let mut client_rooms_lock = client_rooms.lock().await;
        if let Some(client_room_set) = client_rooms_lock.remove(&addr) {
            let mut rooms_lock = rooms.lock().await;
            for room_id in client_room_set {
                if let Some(participants) = rooms_lock.get_mut(&room_id) {
                    participants.remove(&addr);
                    if participants.is_empty() {
                        rooms_lock.remove(&room_id);
                    }
                }
            }
        }
    }
}