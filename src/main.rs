mod server;
mod sim;

#[tokio::main]
async fn main() {
    server::run_server().await;
}
