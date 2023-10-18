use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};
use log::{self, error};
use native_tls::Identity;
use serde::{Deserialize, Serialize};
// use tokio::prelude::*;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{Receiver, Sender},
    time::sleep,
};
use tokio_native_tls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    map::RoomLabel,
    sim::{Player, Simulation},
};

type Tx = UnboundedSender<Message>;
pub type Handle<T> = Arc<Mutex<T>>;
type PeerMap = Handle<HashMap<SocketAddr, Tx>>;

#[derive(Serialize, Deserialize)]
enum PhasmoMessage {
    JoinLobby { name: String },
    ConnectAsAdmin {},
    StartSim {},
    LocationUpdate { name: String, location: RoomLabel },
}

pub struct ServerState {
    peer_map: PeerMap,
    sim: Handle<Simulation>,
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            peer_map: Arc::new(Mutex::new(HashMap::new())),
            sim: Arc::new(Mutex::new(Simulation::new())),
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

    fn register_player(&self, addr: SocketAddr, name: &str) {
        let mut sim = self.sim.lock().unwrap();
        let result = sim.add_player(addr, name);
        drop(sim);

        match result {
            Ok(_) => {
                self.broadcast_gamestate();
                println!("Player registered: {name}")
            }
            Err(e) => println!("{}", e),
        }
    }

    fn handle_message(&self, addr: SocketAddr, msg: Message) {
        match msg {
            Message::Text(msg) => {
                let msg: Result<PhasmoMessage, serde_json::Error> = serde_json::from_str(&msg);
                match msg {
                    Ok(PhasmoMessage::ConnectAsAdmin {}) => {
                        self.send_gamestate(addr);
                    }
                    Ok(PhasmoMessage::JoinLobby { name }) => {
                        self.register_player(addr, &name);
                    }
                    Ok(PhasmoMessage::StartSim {}) => {
                        self.sim.lock().unwrap().start();
                        self.broadcast_gamestate();
                    }
                    Ok(PhasmoMessage::LocationUpdate { name, location }) => {
                        self.sim.lock().unwrap().update_player_loc(&name, location);
                        self.broadcast_gamestate();
                    }
                    _ => println!("Error parsing"),
                }
            }
            _ => (),
        }
    }

    fn send_gamestate(&self, addr: SocketAddr) {
        let mut peer_map = self.peer_map.lock().unwrap();
        let msg = self.get_gamestate();
        match peer_map.get_mut(&addr) {
            Some(sender) => {
                println!("Sending message");
                sender.unbounded_send(msg.clone()).unwrap();
            }
            None => (),
        }
    }

    fn broadcast_gamestate(&self) {
        println!("Broadcasting gamestate");
        self.broadcast(self.get_gamestate());

        self.sim.lock().unwrap().clear_notify_queue();
    }

    fn broadcast(&self, msg: Message) {
        let mut peer_map = self.peer_map.lock().unwrap();
        for peer in peer_map.values_mut() {
            peer.unbounded_send(msg.clone()).unwrap();
        }
    }

    fn broadcast_close(&self) {
        let msg = Message::Close(None);
        self.broadcast(msg);
    }

    fn get_gamestate(&self) -> Message {
        let sim = self.sim.lock().unwrap();

        let gamestate = sim.get_gameupdate();
        let gamestate_ser = serde_json::to_string(&gamestate).unwrap();

        Message::text(gamestate_ser)
    }

    fn update_sim(&self, dt: Duration) -> bool {
        let mut sim = self.sim.lock().unwrap();
        sim.update(dt)
    }

    fn is_started(&self) -> bool {
        self.sim.lock().unwrap().started
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

pub async fn run_server<'a>(rx: Arc<tokio::sync::Mutex<Receiver<()>>>) {
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
    let tls_acceptor = Arc::new(tokio::sync::Mutex::new(
        tokio_native_tls::TlsAcceptor::from(native_acceptor),
    ));

    let sim_state = state.clone();


    let handle1 = tokio::spawn(run_simulation(sim_state));
    let state2 = state.clone();
    let handle2 = tokio::spawn(async move {
        while let Ok((stream, addr)) = listener.accept().await {
            let state = state2.clone();
            let tls_acceptor = tls_acceptor.clone();

            tokio::spawn(handle_connection(state, stream, tls_acceptor, addr));
        }
    });


    let mut rx = rx.lock().await;

    rx.recv().await;
    println!("Closing connections");
    state.lock().unwrap().broadcast_close();

    handle1.abort();
    handle2.abort();
}

pub async fn run_simulation(
    state: Handle<ServerState>
) {
    let fps = 30;
    let dt = Duration::from_millis(1000 / fps);
    loop {
        if state.lock().unwrap().is_started() {
            let changed = state.lock().unwrap().update_sim(dt);
            if changed {
                state.lock().unwrap().broadcast_gamestate();
            }

            sleep(dt).await;
        }
    }
}
