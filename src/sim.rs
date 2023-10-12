use crate::server::{ServerState, Handle};

use tokio::time;

pub async fn run_simulation(state: Handle<ServerState>) {
    loop {
        time::sleep(time::Duration::from_secs(1)).await;
        state.lock().unwrap().inc();
    }
}
