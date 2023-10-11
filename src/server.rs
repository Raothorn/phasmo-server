use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Tx = UnboundedSender<Message>;
type Handle<T> = Arc<Mutex<T>>;
type PeerMap = Handle<HashMap<SocketAddr, Tx>>;

struct ServerState {
    peer_map: PeerMap,
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            peer_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_peer(&self, addr: SocketAddr, tx: Tx) {
        let mut peer_map = self.peer_map.lock().unwrap();
        peer_map.insert(addr, tx);
    }

    fn remove_peer(&self, addr: SocketAddr) {
        let mut peer_map = self.peer_map.lock().unwrap();
        peer_map.remove(&addr).unwrap();
    }

    fn broadcast(&self, msg: Message) {
        let mut peer_map = self.peer_map.lock().unwrap();
        for peer in peer_map.values_mut() {
            peer.unbounded_send(msg.clone()).unwrap();
        }
    }
}

async fn handle_connection(state: Handle<ServerState>, raw_stream: TcpStream, addr: SocketAddr) {
    println!("Incoming TCP connection from: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake occurred");
    println!("WebSocket connection established: {}", addr);

    // Insert the write part of this peer to the peer map.
    let (tx, rx) = unbounded();
    state.lock().unwrap().add_peer(addr, tx);
    
    let (outgoing, incoming) = ws_stream.split();
    
    let broadcast_incoming = incoming.try_for_each(|msg| {
        println!("Received a message from {}: {}", addr, msg.to_text().unwrap());

        state.lock().unwrap().broadcast(msg);

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    state.lock().unwrap().remove_peer(addr);
}

pub async fn run_server() {
    let addr = "192.168.1.199:2000";

    let state = Arc::new(Mutex::new(ServerState::new()));

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    println!("Listening on: {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_connection(state.clone(), stream, addr));
    }
}