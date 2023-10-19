use std::{io, sync::Arc};

mod server;
mod sim;
mod map;
mod ghost;
mod utils;

#[tokio::main]
async fn main() {
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let handle = tokio::spawn(server::run_server(rx));

    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer).unwrap();
    tx.send(()).await.unwrap();
    handle.await.unwrap();
}

