use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::{broadcast, Mutex}};
use crate::websocket::{handler::handle_connection, protocol::{ClientRooms, Clients, RoomMessage, Rooms, Tx}};


pub struct WebSocketServer {
    port: u16,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    tx: Tx
}

impl WebSocketServer {
    pub fn new(port: u16) -> Self {
        let (tx, _rx) = broadcast::channel::<RoomMessage>(100);
        let clients = Arc::new(Mutex::new(HashMap::new()));
        let rooms = Arc::new(Mutex::new(HashMap::new()));
        let client_rooms = Arc::new(Mutex::new(HashMap::new()));

        Self {
            port,
            clients,
            rooms,
            client_rooms,
            tx
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("127.0.0.1:{}", self.port);
        println!("📡 WebSocket server listening on: {}", addr);

        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => return Err(e.into())
        };

        while let Ok((stream, addr)) = listener.accept().await {
            let tx_clone = self.tx.clone();
            let clients_clone = self.clients.clone();
            let client_rooms_clone = self.client_rooms.clone();
            let rooms_clone = self.rooms.clone();

            tokio::spawn(handle_connection(
                stream,
                addr,
                tx_clone,
                clients_clone,
                rooms_clone,
                client_rooms_clone
            ));


        }
        Ok(())

    }
}