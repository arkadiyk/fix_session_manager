use std::time::Duration;

/// Configuration for a FIX session
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// SenderCompID (tag 49)
    pub sender_comp_id: String,
    
    /// TargetCompID (tag 56)
    pub target_comp_id: String,
    
    /// Heartbeat interval in seconds
    pub heartbeat_interval: Duration,
    
    /// FIX version (e.g., "FIX.4.4")
    pub begin_string: String,
    
    /// Initial outbound sequence number
    pub outbound_seqnum: u64,
    
    /// Initial inbound sequence number
    pub inbound_seqnum: u64,
    
    /// Interval to wait between reconnection attempts
    pub reconnect_interval: Duration,
}

impl SessionConfig {
    /// Creates a new session configuration with default values
    pub fn new(sender: &str, target: &str) -> Self {
        Self {
            sender_comp_id: sender.to_string(),
            target_comp_id: target.to_string(),
            heartbeat_interval: Duration::from_secs(30),
            begin_string: "FIX.4.4".to_string(),
            outbound_seqnum: 1,
            inbound_seqnum: 1,
            reconnect_interval: Duration::from_secs(5),
        }
    }
    
    /// Creates a unique session identifier
    pub fn session_id(&self) -> String {
        format!("{}:{}", self.sender_comp_id, self.target_comp_id)
    }
}