use tokio::sync::Mutex;
use std::collections::HashSet;
use std::time::{Instant, Duration};
use crate::error::FixError;
use crate::message::{FixMessage, message_types};

/// State of session recovery
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryState {
    /// Normal operation, no recovery in progress
    Normal,
    /// Waiting for response to resend request
    AwaitingResend,
    /// Processing resent messages
    ProcessingResend,
    /// Recovering from disconnect
    ReconnectRecovery,
}

/// Manages recovery operations for a FIX session
pub struct RecoveryManager {
    /// Current recovery state
    state: Mutex<RecoveryState>,
    
    /// Set of message types to skip during recovery (administrative messages)
    admin_message_types: HashSet<String>,
    
    /// Timeout for recovery operations
    recovery_timeout: Duration,
    
    /// Time when recovery started
    recovery_start_time: Mutex<Option<Instant>>,
    
    /// The highest sequence number processed during recovery
    highest_resent_seqnum: Mutex<u64>,
}

impl RecoveryManager {
    /// Create a new recovery manager
    pub fn new() -> Self {
        // Initialize with admin message types
        let mut admin_types = HashSet::new();
        admin_types.insert(message_types::HEARTBEAT.to_string());
        admin_types.insert(message_types::TEST_REQUEST.to_string());
        admin_types.insert(message_types::RESEND_REQUEST.to_string());
        admin_types.insert(message_types::REJECT.to_string());
        admin_types.insert(message_types::SEQUENCE_RESET.to_string());
        admin_types.insert(message_types::LOGOUT.to_string());
        admin_types.insert(message_types::LOGON.to_string());
        
        Self {
            state: Mutex::new(RecoveryState::Normal),
            admin_message_types: admin_types,
            recovery_timeout: Duration::from_secs(60),
            recovery_start_time: Mutex::new(None),
            highest_resent_seqnum: Mutex::new(0),
        }
    }
    
    /// Start recovery due to sequence number gap
    pub async fn start_gap_recovery(&self, _expected_seq: u64, _received_seq: u64) -> Result<(), FixError> {
        let mut state = self.state.lock().await;
        let mut recovery_time = self.recovery_start_time.lock().await;
        
        *state = RecoveryState::AwaitingResend;
        *recovery_time = Some(Instant::now());
        
        // Reset highest resent sequence number
        let mut highest = self.highest_resent_seqnum.lock().await;
        *highest = 0;
        
        Ok(())
    }
    
    /// Start recovery due to disconnect/reconnect
    pub async fn start_reconnect_recovery(&self) -> Result<(), FixError> {
        let mut state = self.state.lock().await;
        let mut recovery_time = self.recovery_start_time.lock().await;
        
        *state = RecoveryState::ReconnectRecovery;
        *recovery_time = Some(Instant::now());
        
        // Reset highest resent sequence number
        let mut highest = self.highest_resent_seqnum.lock().await;
        *highest = 0;
        
        Ok(())
    }
    
    /// Process a resent message
    pub async fn process_resent_message(&self, message: &FixMessage) -> Result<bool, FixError> {
        // Check if this is an admin message that should be skipped
        if let Some(msg_type) = message.get_field(crate::message::tags::MSG_TYPE) {
            if self.admin_message_types.contains(msg_type) {
                // Skip admin messages during resend
                return Ok(false);
            }
        }
        
        // Update highest resent sequence number
        if let Ok(seq_num) = message.seq_num() {
            let mut highest = self.highest_resent_seqnum.lock().await;
            if seq_num > *highest {
                *highest = seq_num;
            }
        }
        
        // Process the application message
        Ok(true)
    }
    
    /// Complete recovery
    pub async fn complete_recovery(&self) -> Result<u64, FixError> {
        let mut state = self.state.lock().await;
        let mut recovery_time = self.recovery_start_time.lock().await;
        let highest = *self.highest_resent_seqnum.lock().await;
        
        *state = RecoveryState::Normal;
        *recovery_time = None;
        
        Ok(highest)
    }
    
    /// Check if recovery has timed out
    pub async fn check_timeout(&self) -> bool {
        if let Some(start_time) = *self.recovery_start_time.lock().await {
            if Instant::now().duration_since(start_time) > self.recovery_timeout {
                return true;
            }
        }
        
        false
    }
    
    /// Get current recovery state
    pub async fn get_state(&self) -> RecoveryState {
        match *self.state.lock().await {
            RecoveryState::Normal => RecoveryState::Normal,
            RecoveryState::AwaitingResend => RecoveryState::AwaitingResend,
            RecoveryState::ProcessingResend => RecoveryState::ProcessingResend,
            RecoveryState::ReconnectRecovery => RecoveryState::ReconnectRecovery,
        }
    }
    
    /// Check if we're currently in recovery
    pub async fn is_in_recovery(&self) -> bool {
        match *self.state.lock().await {
            RecoveryState::Normal => false,
            _ => true,
        }
    }
    
    /// Set recovery state to processing resend
    pub async fn set_processing_resend(&self) -> Result<(), FixError> {
        let mut state = self.state.lock().await;
        *state = RecoveryState::ProcessingResend;
        Ok(())
    }
    
    /// Set recovery timeout
    pub fn set_recovery_timeout(&mut self, timeout: Duration) {
        self.recovery_timeout = timeout;
    }
}