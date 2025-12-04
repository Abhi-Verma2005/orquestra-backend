use std::{collections::HashMap, sync::Arc};
use tokio::{net::TcpListener, sync::{broadcast, Mutex}};
use crate::websocket::{handler::handle_connection, protocol::{ClientRooms, Clients, RoomMessage, Rooms, Tx, UserClients, WalletClients, RoomUserIds, UserNames}};
use crate::orchestrator::service::OrchestratorService;

pub struct WebSocketServer {
    port: u16,
    clients: Clients,
    rooms: Rooms,
    client_rooms: ClientRooms,
    user_clients: UserClients,
    wallet_clients: WalletClients,
    room_user_ids: RoomUserIds,
    user_names: UserNames,
    tx: Tx,
    orchestrator: Arc<OrchestratorService>,
}

impl WebSocketServer {
    pub fn new(port: u16, orchestrator: OrchestratorService) -> Self {
        let (tx, _rx) = broadcast::channel::<RoomMessage>(100);
        let clients = Arc::new(Mutex::new(HashMap::new()));
        let rooms = Arc::new(Mutex::new(HashMap::new()));
        let client_rooms = Arc::new(Mutex::new(HashMap::new()));
        let user_clients = Arc::new(Mutex::new(HashMap::new()));
        let wallet_clients = Arc::new(Mutex::new(HashMap::new()));
        let room_user_ids = Arc::new(Mutex::new(HashMap::new()));
        let user_names = Arc::new(Mutex::new(HashMap::new()));

        Self {
            port,
            clients,
            rooms,
            client_rooms,
            user_clients,
            wallet_clients,
            room_user_ids,
            user_names,
            tx,
            orchestrator: Arc::new(orchestrator),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Bind to 0.0.0.0 to accept connections from any interface (needed for deployment)
        let addr = format!("0.0.0.0:{}", self.port);

        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => return Err(e.into())
        };

        while let Ok((stream, addr)) = listener.accept().await {
            let tx_clone = self.tx.clone();
            let clients_clone = self.clients.clone();
            let client_rooms_clone = self.client_rooms.clone();
            let rooms_clone = self.rooms.clone();
            let user_clients_clone = self.user_clients.clone();
            let wallet_clients_clone = self.wallet_clients.clone();
            let room_user_ids_clone = self.room_user_ids.clone();
            let user_names_clone = self.user_names.clone();
            let orchestrator_clone = self.orchestrator.clone();

            tokio::spawn(handle_connection(
                stream,
                addr,
                tx_clone,
                clients_clone,
                rooms_clone,
                client_rooms_clone,
                user_clients_clone,
                wallet_clients_clone,
                room_user_ids_clone,
                user_names_clone,
                orchestrator_clone,
            ));
        }
        Ok(())
    }
}