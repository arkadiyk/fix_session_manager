use std::time::{Instant, Duration};
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::error::FixError;

/// Manages heartbeat timing for a FIX session
pub struct HeartbeatMonitor {
    /// Last time a message was received
    last_received: Mutex<Instant>,
    
    /// Last time a message was sent
    last_sent: Mutex<Instant>,
    
    /// Heartbeat interval in seconds
    heartbeat_interval: Duration,
    
    /// Pending test request IDs
    pending_test_requests: Mutex<Vec<(String, Instant)>>,
    
    /// Number of missed heartbeats before considering the connection dead
    missed_heartbeat_threshold: u32,
    
    /// Test request timeout multiplier (relative to heartbeat interval)
    test_request_timeout_mult: f32,
}

impl HeartbeatMonitor {
    /// Create a new heartbeat monitor with the specified interval
    pub fn new(heartbeat_interval: Duration) -> Self {
        let now = Instant::now();
        Self {
            last_received: Mutex::new(now),
            last_sent: Mutex::new(now),
            heartbeat_interval,
            pending_test_requests: Mutex::new(Vec::new()),
            missed_heartbeat_threshold: 3,
            test_request_timeout_mult: 1.5,
        }
    }
    
    /// Update the last received message timestamp
    pub async fn message_received(&self) {
        let mut last_received = self.last_received.lock().await;
        *last_received = Instant::now();
    }
    
    /// Update the last sent message timestamp
    pub async fn message_sent(&self) {
        let mut last_sent = self.last_sent.lock().await;
        *last_sent = Instant::now();
    }
    
    /// Check if a heartbeat should be sent
    pub async fn should_send_heartbeat(&self) -> bool {
        let last_sent = self.last_sent.lock().await;
        Instant::now().duration_since(*last_sent) >= self.heartbeat_interval
    }
    
    /// Check if a test request should be sent
    pub async fn should_send_test_request(&self) -> bool {
        let last_received = self.last_received.lock().await;
        let elapsed = Instant::now().duration_since(*last_received);
        let threshold = self.heartbeat_interval.mul_f32(self.missed_heartbeat_threshold as f32);
        
        elapsed >= threshold
    }
    
    /// Generate a unique test request ID
    pub async fn generate_test_request_id(&self) -> String {
        let id = Uuid::new_v4().to_string();
        let mut pending = self.pending_test_requests.lock().await;
        pending.push((id.clone(), Instant::now()));
        id
    }
    
    /// Check if there are test requests that have timed out
    pub async fn check_test_request_timeout(&self) -> bool {
        let mut pending = self.pending_test_requests.lock().await;
        let now = Instant::now();
        let timeout_duration = self.heartbeat_interval.mul_f32(self.test_request_timeout_mult);
        
        // Check if any test requests have timed out
        let timed_out = pending.iter().any(|(_, time)| now.duration_since(*time) > timeout_duration);
        
        // Clean up old test requests regardless of timeout status
        pending.retain(|(_, time)| now.duration_since(*time) <= timeout_duration);
        
        timed_out
    }
    
    /// Acknowledge receipt of a test request response
    pub async fn acknowledge_test_request(&self, test_req_id: &str) -> Result<bool, FixError> {
        let mut pending = self.pending_test_requests.lock().await;
        
        let found = pending.iter().position(|(id, _)| id == test_req_id);
        
        if let Some(idx) = found {
            pending.remove(idx);
            Ok(true)
        } else {
            // We received a response to a test request we didn't send
            Ok(false)
        }
    }
    
    /// Set the threshold for missed heartbeats
    pub fn set_missed_heartbeat_threshold(&mut self, threshold: u32) {
        self.missed_heartbeat_threshold = threshold;
    }
    
    /// Get the heartbeat interval
    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }
    
    /// Check if the session is inactive (no messages received) for too long
    pub async fn is_session_inactive(&self) -> bool {
        let last_received = self.last_received.lock().await;
        let elapsed = Instant::now().duration_since(*last_received);
        let threshold = self.heartbeat_interval.mul_f32(
            (self.missed_heartbeat_threshold as f32) * self.test_request_timeout_mult
        );
        
        elapsed >= threshold
    }
}