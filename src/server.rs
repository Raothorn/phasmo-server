use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};
use native_tls::Identity;
use serde::Serialize;
// use tokio::prelude::*;
use tokio::net::{TcpListener, TcpStream};
use tokio_native_tls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

use crate::sim::run_simulation;

type Tx = UnboundedSender<Message>;
pub type Handle<T> = Arc<Mutex<T>>;
type PeerMap = Handle<HashMap<SocketAddr, Tx>>;

#[derive(Serialize)]
enum GameState {
    Lobby { players: Vec<String>, val: u32 },
}

struct Player {
    name: String,
    addr: SocketAddr,
}

pub struct ServerState {
    peer_map: PeerMap,
    players: Handle<Vec<Player>>,
    sim: Handle<u32>
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            peer_map: Arc::new(Mutex::new(HashMap::new())),
            players: Arc::new(Mutex::new(Vec::new())),
            sim: Arc::new(Mutex::new(0))
        }
    }

    fn add_peer(&self, addr: SocketAddr, tx: Tx) {
        let mut peer_map = self.peer_map.lock().unwrap();
        peer_map.insert(addr, tx);
    }

    // Temp
    pub fn inc(&self) {
        let mut sim = self.sim.lock().unwrap();
        *sim += 1;
    }

    fn remove_peer(&self, addr: SocketAddr) {
        let mut peer_map = self.peer_map.lock().unwrap();
        peer_map.remove(&addr).unwrap();
    }

    fn register_player(&self, addr: SocketAddr, name: &str) {
        let mut players = self.players.lock().unwrap();

        if players.iter().any(|p| p.addr == addr) {
            println!("Player already in lobby");
        } else if players.iter().any(|p| p.name == name) {
            println!("Name already taken");
        } else {
            println!("Adding player {} to lobby", name);
            let player = Player {
                name: name.to_owned(),
                addr,
            };
            players.push(player);
            drop(players);

            println!("broadcasting");
            self.broadcast_gamestate();
            println!("FInished broadcasting");
        }
    }

    fn handle_message(&self, addr: SocketAddr, msg: Message) {
        match msg {
            Message::Text(msg) => {
                let msg_split: Vec<&str> = msg.split(' ').collect();
                let cmd = msg_split[0];

                if cmd == "JoinLobby" {
                    let name = msg_split[1];
                    self.register_player(addr, name);
                }
            }
            _ => (),
        }
    }

    fn broadcast_gamestate(&self) {
        let players = self.players.lock().unwrap();
        let player_names = players.iter().map(|p| p.name.clone()).collect();

        let val = *self.sim.lock().unwrap();

        let gamestate = GameState::Lobby {
            players: player_names,
            val
        };
        let gamestate_ser = serde_json::to_string(&gamestate).unwrap();

        let msg = Message::text(gamestate_ser);
        self.broadcast(msg);
    }

    fn broadcast(&self, msg: Message) {
        let mut peer_map = self.peer_map.lock().unwrap();
        for peer in peer_map.values_mut() {
            peer.unbounded_send(msg.clone()).unwrap();
        }
    }
}

async fn handle_connection(
    state: Handle<ServerState>,
    raw_stream: TcpStream,
    acceptor: Arc<tokio::sync::Mutex<TlsAcceptor>>,
    addr: SocketAddr,
) {
    println!("Incoming TCP connection from: {}", addr);


    let acceptor = acceptor.lock().await;
    let stream = acceptor.accept(raw_stream).await;
    drop(acceptor);

    match stream {
        Ok(stream) => {
            let ws_stream = tokio_tungstenite::accept_async(stream).await;
            match ws_stream {
                Ok(ws_stream) => {
                    println!("WebSocket connection established: {}", addr);

                    // Insert the write part of this peer to the peer map.
                    let (tx, rx) = unbounded();
                    state.lock().unwrap().add_peer(addr, tx);

                    let (outgoing, incoming) = ws_stream.split();

                    let handle_incoming = incoming.try_for_each(|msg| {
                        println!(
                            "Received a message from {}: {}",
                            addr,
                            msg.to_text().unwrap()
                        );

                        state.lock().unwrap().handle_message(addr, msg);

                        future::ok(())
                    });

                    let receive_from_others = rx.map(Ok).forward(outgoing);

                    pin_mut!(handle_incoming, receive_from_others);
                    future::select(handle_incoming, receive_from_others).await;

                    println!("{} disconnected", &addr);
                    state.lock().unwrap().remove_peer(addr);
                }
                Err(e) => println!("{}", e),
            }
        }
        Err(e) => println!("{}", e),
    }
}

pub async fn run_server() {
    let addr = "192.168.1.199:2000";

    let state = Arc::new(Mutex::new(ServerState::new()));

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    println!("Listening on: {}", addr);

    // TLS
    let der = include_bytes!("secrets/keyStore.p12");
    let cert = Identity::from_pkcs12(der, "pass").unwrap();
    let native_acceptor = native_tls::TlsAcceptor::builder(cert).build().unwrap();
    let tls_acceptor = Arc::new(tokio::sync::Mutex::new(tokio_native_tls::TlsAcceptor::from(
        native_acceptor,
    )));

    let sim_state = state.clone();
    tokio::spawn(run_simulation(sim_state));

    while let Ok((stream, addr)) = listener.accept().await {
        let state = state.clone();
        let tls_acceptor = tls_acceptor.clone();

        tokio::spawn(handle_connection(state, stream, tls_acceptor, addr));
    }

}
