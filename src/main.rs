mod message;
mod session;
mod error;
mod config;
mod heartbeat;
mod sequence;
mod recovery;

use session::SessionManager;
use config::SessionConfig;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    tracing_subscriber::fmt::init();
    
    // Example configuration for a FIX session
    let config = SessionConfig {
        sender_comp_id: "SENDER".to_string(),
        target_comp_id: "TARGET".to_string(),
        heartbeat_interval: std::time::Duration::from_secs(30),
        begin_string: "FIX.4.4".to_string(),
        outbound_seqnum: 1,
        inbound_seqnum: 1,
        reconnect_interval: std::time::Duration::from_secs(5),
    };
    
    // Create and start a session manager
    let mut session_manager = SessionManager::new(config);
    
    // In a real application, you'd integrate this with your network I/O
    match session_manager.start().await {
        Ok(_) => info!("Session manager started successfully"),
        Err(e) => error!("Failed to start session manager: {}", e),
    }
    
    // For demonstration purposes, we'll just sleep for a while
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    
    Ok(())
}
