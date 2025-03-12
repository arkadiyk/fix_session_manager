use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Duration};
use bytes::{BytesMut, Bytes};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;

use crate::config::SessionConfig;
use crate::error::FixError;
use crate::heartbeat::HeartbeatMonitor;
use crate::message::{FixMessage, message_types, tags};
use crate::recovery::{RecoveryManager, RecoveryState};
use crate::sequence::SequenceManager;

/// Message direction
pub enum MessageDirection {
    /// Inbound message (received)
    Inbound,
    
    /// Outbound message (sent)
    Outbound,
}

/// Session state
#[derive(Debug, Clone, PartialEq, Copy)]
pub enum SessionState {
    /// Initial state
    Created,
    
    /// Connecting to counterparty
    Connecting,
    
    /// Logon sent, waiting for response
    LogonSent,
    
    /// Session established and active
    Active,
    
    /// Logout sent, waiting for confirmation
    LogoutSent,
    
    /// Session disconnected
    Disconnected,
    
    /// Fatal error occurred
    Error,
}

/// Callback trait for session events
#[async_trait]
pub trait SessionCallback: Send + Sync {
    /// Called when a message is received
    async fn on_message(&self, message: FixMessage) -> Result<(), FixError>;
    
    /// Called when the session state changes
    async fn on_state_change(&self, old_state: SessionState, new_state: SessionState);
    
    /// Called when a transport-level error occurs
    async fn on_error(&self, error: FixError);
}

/// Message with context for internal processing
struct MessageContext {
    /// The FIX message
    message: FixMessage,
    
    /// Direction (inbound/outbound)
    direction: MessageDirection,
    
    /// Raw message bytes
    raw_data: Bytes,
}

/// Main FIX session manager
pub struct SessionManager {
    /// Session configuration
    config: SessionConfig,
    
    /// Current session state
    state: Mutex<SessionState>,
    
    /// Sequence number manager
    seq_manager: Arc<SequenceManager>,
    
    /// Heartbeat monitor
    heartbeat: Arc<HeartbeatMonitor>,
    
    /// Recovery manager
    recovery: Arc<RecoveryManager>,
    
    /// Message sender channel
    msg_sender: mpsc::Sender<Bytes>,
    
    /// Message receiver channel
    msg_receiver: mpsc::Receiver<Bytes>,
    
    /// Session callback
    callback: Option<Arc<dyn SessionCallback>>,
}

impl SessionManager {
    /// Create a new session manager with the provided configuration
    pub fn new(config: SessionConfig) -> Self {
        let (tx, rx) = mpsc::channel::<Bytes>(1000); // Channel for outgoing messages
        
        let seq_manager = Arc::new(SequenceManager::new(
            config.outbound_seqnum,
            config.inbound_seqnum
        ));
        
        let heartbeat = Arc::new(HeartbeatMonitor::new(config.heartbeat_interval));
        let recovery = Arc::new(RecoveryManager::new());
        
        Self {
            config,
            state: Mutex::new(SessionState::Created),
            seq_manager,
            heartbeat,
            recovery,
            msg_sender: tx,
            msg_receiver: rx,
            callback: None,
        }
    }
    
    /// Set the session callback
    pub fn set_callback<T: SessionCallback + 'static>(&mut self, callback: T) {
        self.callback = Some(Arc::new(callback));
    }
    
    /// Start the session manager
    pub async fn start(&mut self) -> Result<(), FixError> {
        self.transition_state(SessionState::Connecting).await;
        
        // Start the background tasks
        self.start_heartbeat_task();
        
        // Initiate logon
        self.send_logon().await?;
        self.transition_state(SessionState::LogonSent).await;
        
        Ok(())
    }
    
    /// Handle an incoming message
    pub async fn handle_message(&self, data: &[u8]) -> Result<(), FixError> {
        // Parse the message
        let message = match FixMessage::from_bytes(data) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to parse message: {}", e);
                return Err(e);
            }
        };
        
        // Always update the heartbeat timestamp on receiving any message
        self.heartbeat.message_received().await;
        
        // Process the message based on type
        match message.msg_type() {
            // Session-level messages
            message_types::LOGON => self.handle_logon(&message).await?,
            message_types::LOGOUT => self.handle_logout(&message).await?,
            message_types::HEARTBEAT => self.handle_heartbeat(&message).await?,
            message_types::TEST_REQUEST => self.handle_test_request(&message).await?,
            message_types::RESEND_REQUEST => self.handle_resend_request(&message).await?,
            message_types::SEQUENCE_RESET => self.handle_sequence_reset(&message).await?,
            message_types::REJECT => self.handle_reject(&message).await?,
            
            // Application messages
            _ => self.handle_application_message(&message, data).await?,
        }
        
        Ok(())
    }
    
    /// Send a message through the session
    pub async fn send_message(&self, message: FixMessage) -> Result<(), FixError> {
        let state = self.state.lock().await;
        
        // Only allow sending messages when active (except for Logon and admin messages in certain states)
        if *state != SessionState::Active && message.msg_type() != message_types::LOGON {
            if !self.is_admin_message(&message) || *state == SessionState::Error || *state == SessionState::Disconnected {
                return Err(FixError::ProtocolViolation(
                    format!("Cannot send message in {} state", format!("{:?}", *state))
                ));
            }
        }
        
        // Get the next sequence number
        let seq_num = self.seq_manager.next_outbound_seqnum();
        
        // Convert to bytes
        let bytes = message.to_bytes(
            &self.config.begin_string,
            &self.config.sender_comp_id,
            &self.config.target_comp_id,
            seq_num
        );
        
        // Send the bytes
        if let Err(_) = self.msg_sender.send(bytes.freeze()).await {
            return Err(FixError::ConnectionLost("Failed to send message".to_string()));
        }
        
        // Update heartbeat tracking
        self.heartbeat.message_sent().await;
        
        Ok(())
    }
    
    /// Check if a message is an administrative message
    fn is_admin_message(&self, message: &FixMessage) -> bool {
        match message.msg_type() {
            message_types::LOGON | message_types::LOGOUT | message_types::HEARTBEAT |
            message_types::TEST_REQUEST | message_types::RESEND_REQUEST |
            message_types::SEQUENCE_RESET | message_types::REJECT => true,
            _ => false,
        }
    }
    
    /// Handle a logon message
    async fn handle_logon(&self, message: &FixMessage) -> Result<(), FixError> {
        let mut state = self.state.lock().await;
        
        match *state {
            SessionState::LogonSent => {
                // Validate the logon message
                if let Err(e) = self.validate_logon_message(message) {
                    *state = SessionState::Error;
                    return Err(e);
                }
                
                // Successful logon
                *state = SessionState::Active;
                info!("Session established");
                
                // If we have a callback, notify it
                if let Some(callback) = &self.callback {
                    callback.on_state_change(SessionState::LogonSent, SessionState::Active).await;
                }
                
                Ok(())
            },
            SessionState::Connecting => {
                // Received a logon before we sent one
                // This is acceptable in some cases - send a logon response
                
                // Validate the logon message
                if let Err(e) = self.validate_logon_message(message) {
                    *state = SessionState::Error;
                    return Err(e);
                }
                
                // Send logon response
                let hb_interval_secs = self.heartbeat.heartbeat_interval().as_secs();
                let logon_msg = FixMessage::logon(hb_interval_secs, false);
                drop(state); // Release lock before async call
                self.send_message(logon_msg).await?;
                
                // Update state
                self.transition_state(SessionState::Active).await;
                info!("Session established from remote logon");
                
                Ok(())
            },
            _ => {
                warn!("Received logon in invalid state: {:?}", *state);
                Err(FixError::ProtocolViolation(format!(
                    "Received logon in invalid state: {:?}", 
                    *state
                )))
            }
        }
    }
    
    /// Validate a logon message
    fn validate_logon_message(&self, message: &FixMessage) -> Result<(), FixError> {
        // Check required fields
        if message.get_field(tags::HEARTBEAT_INT).is_none() {
            return Err(FixError::InvalidFormat("Missing HeartbeatInterval".to_string()));
        }
        
        // Check if this is a reset request
        if let Some(val) = message.get_field(tags::RESET_SEQ_NUM_FLAG) {
            if val == "Y" {
                // Reset sequence numbers
                self.seq_manager.reset(1, 1);
                debug!("Sequence numbers reset request accepted");
            }
        }
        
        Ok(())
    }
    
    /// Handle a logout message
    async fn handle_logout(&self, _message: &FixMessage) -> Result<(), FixError> {
        let mut state = self.state.lock().await;
        
        match *state {
            SessionState::Active => {
                // Normal logout - send a logout response
                let logout_msg = FixMessage::logout(None);
                drop(state); // Release lock before async call
                
                // We need to respond to a logout
                self.send_message(logout_msg).await?;
                
                // Update state
                self.transition_state(SessionState::Disconnected).await;
                info!("Session logged out gracefully");
                
                Ok(())
            },
            SessionState::LogoutSent => {
                // We were already logging out, this is the confirmation
                *state = SessionState::Disconnected;
                info!("Session logout completed");
                
                // If we have a callback, notify it
                if let Some(callback) = &self.callback {
                    callback.on_state_change(SessionState::LogoutSent, SessionState::Disconnected).await;
                }
                
                Ok(())
            },
            _ => {
                warn!("Received logout in invalid state: {:?}", *state);
                *state = SessionState::Disconnected;
                
                // If we have a callback, notify it
                if let Some(callback) = &self.callback {
                    let old_state = *state;
                    callback.on_state_change(old_state, SessionState::Disconnected).await;
                }
                
                Ok(())
            }
        }
    }
    
    /// Handle a heartbeat message
    async fn handle_heartbeat(&self, message: &FixMessage) -> Result<(), FixError> {
        // Update last received time (already done in handle_message)
        
        // If this is a response to a test request, acknowledge it
        if let Some(test_req_id) = message.get_field(tags::TEST_REQ_ID) {
            self.heartbeat.acknowledge_test_request(test_req_id).await?;
            debug!("Received heartbeat response to test request: {}", test_req_id);
        } else {
            debug!("Received heartbeat");
        }
        
        Ok(())
    }
    
    /// Handle a test request message
    async fn handle_test_request(&self, message: &FixMessage) -> Result<(), FixError> {
        // A test request requires a heartbeat response with the same TestReqID
        if let Some(test_req_id) = message.get_field(tags::TEST_REQ_ID) {
            debug!("Received test request: {}", test_req_id);
            
            // Send heartbeat response
            let heartbeat = FixMessage::heartbeat(Some(test_req_id));
            self.send_message(heartbeat).await?;
            
            debug!("Sent heartbeat in response to test request");
        } else {
            warn!("Received test request without TestReqID");
            // This is a protocol violation but we'll be lenient
        }
        
        Ok(())
    }
    
    /// Handle a resend request message
    async fn handle_resend_request(&self, message: &FixMessage) -> Result<(), FixError> {
        // Extract the sequence range
        let begin_seq = message.get_field(tags::BEGIN_SEQ_NO)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(FixError::InvalidFormat("Missing or invalid BeginSeqNo".to_string()))?;
            
        let end_seq = message.get_field(tags::END_SEQ_NO)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(FixError::InvalidFormat("Missing or invalid EndSeqNo".to_string()))?;
            
        debug!("Received resend request for messages {} to {}", begin_seq, end_seq);
            
        // In a real implementation, we would retrieve these messages from storage
        // For now, we'll send a sequence reset as a gap fill
        let reset_seq = self.seq_manager.current_outbound_seqnum();
        let seq_reset = FixMessage::sequence_reset(reset_seq, true);
        self.send_message(seq_reset).await?;
        
        debug!("Sent sequence reset to {} as gap fill", reset_seq);
        
        Ok(())
    }
    
    /// Handle a sequence reset message
    async fn handle_sequence_reset(&self, message: &FixMessage) -> Result<(), FixError> {
        // Extract the new sequence number
        let new_seq_no = message.get_field(tags::NEW_SEQ_NO)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(FixError::InvalidFormat("Missing or invalid NewSeqNo".to_string()))?;
            
        // Determine if this is a gap fill
        let gap_fill = message.get_field(tags::GAP_FILL_FLAG)
            .map(|s| s == "Y")
            .unwrap_or(false);
            
        debug!("Received sequence reset to {}, gap fill: {}", new_seq_no, gap_fill);
        
        // Update our expected sequence number
        if new_seq_no >= self.seq_manager.expected_inbound_seqnum() {
            // We need to reset the sequence manager's expected inbound sequence
            // This is a simplification as we're using atomic values
            self.seq_manager.reset(
                self.seq_manager.current_outbound_seqnum(),
                new_seq_no
            );
            
            debug!("Reset expected inbound sequence number to {}", new_seq_no);
        } else {
            warn!("Received sequence reset with invalid sequence number");
            // Protocol violation, but we'll be lenient
        }
        
        Ok(())
    }
    
    /// Handle a reject message
    async fn handle_reject(&self, message: &FixMessage) -> Result<(), FixError> {
        // Extract reject information
        let seq_num = message.get_field(tags::MSG_SEQ_NUM)
            .and_then(|s| s.parse::<u64>().ok());
            
        if let Some(seq) = seq_num {
            warn!("Received reject for message {}", seq);
        } else {
            warn!("Received reject without valid sequence number");
        }
        
        // In a production system, we might take additional actions here
        Ok(())
    }
    
    /// Handle an application (non-session) message
    async fn handle_application_message(&self, message: &FixMessage, raw_data: &[u8]) -> Result<(), FixError> {
        // Process sequence number
        let seq_num = message.seq_num()?;
        let seq_ok = self.seq_manager.process_inbound_seqnum(seq_num).await?;
        
        if !seq_ok {
            // Gap detected, start recovery
            let expected = self.seq_manager.expected_inbound_seqnum();
            info!("Sequence gap detected: expected {}, got {}", expected, seq_num);
            
            // Start recovery process
            self.recovery.start_gap_recovery(expected, seq_num).await?;
            
            // Buffer the message
            self.seq_manager.buffer_message(seq_num, raw_data.to_vec()).await?;
            
            // Request resend of missing messages
            let resend_req = FixMessage::resend_request(expected, seq_num - 1);
            self.send_message(resend_req).await?;
            
            // Don't forward this message yet
            return Ok(());
        }
        
        // If in recovery, check if this is a response to our resend request
        if let RecoveryState::AwaitingResend = self.recovery.get_state().await {
            // Check if we need to process any buffered messages
            let buffered = self.seq_manager.process_buffered_messages().await;
            
            if !buffered.is_empty() {
                debug!("Processing {} buffered messages", buffered.len());
                
                // Process each buffered message
                for msg_data in buffered {
                    let buffered_msg = FixMessage::from_bytes(&msg_data)?;
                    
                    // Forward to application
                    if let Some(callback) = &self.callback {
                        callback.on_message(buffered_msg).await?;
                    }
                }
                
                // Complete recovery if all expected messages are received
                self.recovery.complete_recovery().await?;
            }
        }
        
        // Forward message to application
        if let Some(callback) = &self.callback {
            callback.on_message(message.clone()).await?;
        }
        
        Ok(())
    }
    
    /// Send a logon message
    async fn send_logon(&self) -> Result<(), FixError> {
        // Create a logon message
        let heartbeat_secs = self.heartbeat.heartbeat_interval().as_secs();
        let logon = FixMessage::logon(heartbeat_secs, false);
        
        // Send the message
        self.send_message(logon).await
    }
    
    /// Start the heartbeat task
    fn start_heartbeat_task(&self) {
        // Clone the necessary components for the task
        let heartbeat = self.heartbeat.clone();
        let msg_sender = self.msg_sender.clone();
        let session_id = self.config.session_id();
        let seq_manager = self.seq_manager.clone();
        
        // Spawn a tokio task for heartbeat monitoring
        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(1));
            
            loop {
                // Wait for the next interval tick
                check_interval.tick().await;
                
                // Check if it's time to send a heartbeat
                if heartbeat.should_send_heartbeat().await {
                    let hb = FixMessage::heartbeat(None);
                    let seq_num = seq_manager.next_outbound_seqnum();
                    
                    let heartbeat_bytes = hb.to_bytes(
                        "FIX.4.4", 
                        &session_id.split(':').next().unwrap_or("SENDER"), 
                        &session_id.split(':').nth(1).unwrap_or("TARGET"), 
                        seq_num
                    );
                    
                    // Send the heartbeat
                    if let Err(e) = msg_sender.send(heartbeat_bytes.freeze()).await {
                        error!("Failed to send heartbeat: {}", e);
                        break;
                    }
                    
                    // Update heartbeat tracking
                    heartbeat.message_sent().await;
                    debug!("Sent heartbeat, seq: {}", seq_num);
                }
                
                // Check if we need to send a test request
                if heartbeat.should_send_test_request().await {
                    let test_req_id = heartbeat.generate_test_request_id().await;
                    let test_req = FixMessage::test_request(&test_req_id);
                    let seq_num = seq_manager.next_outbound_seqnum();
                    
                    let test_req_bytes = test_req.to_bytes(
                        "FIX.4.4", 
                        &session_id.split(':').next().unwrap_or("SENDER"), 
                        &session_id.split(':').nth(1).unwrap_or("TARGET"), 
                        seq_num
                    );
                    
                    // Send the test request
                    if let Err(e) = msg_sender.send(test_req_bytes.freeze()).await {
                        error!("Failed to send test request: {}", e);
                        break;
                    }
                    
                    // Update heartbeat tracking
                    heartbeat.message_sent().await;
                    debug!("Sent test request, ID: {}, seq: {}", test_req_id, seq_num);
                    
                    // Check if any test requests have timed out
                    if heartbeat.check_test_request_timeout().await {
                        error!("Test request timeout - connection may be lost");
                        break;
                    }
                }
            }
        });
    }
    
    /// Transition to a new session state
    async fn transition_state(&self, new_state: SessionState) {
        let mut state = self.state.lock().await;
        let old_state = std::mem::replace(&mut *state, new_state.clone());
        
        // Log the transition
        debug!("Session state change: {:?} -> {:?}", old_state, new_state);
        
        // Notify callback if set
        if let Some(callback) = &self.callback {
            callback.on_state_change(old_state, new_state).await;
        }
    }
    
    /// Stop the session gracefully
    pub async fn stop(&self) -> Result<(), FixError> {
        let state = self.state.lock().await;
        
        if *state == SessionState::Active {
            drop(state); // Release the lock before awaiting
            
            // Send logout
            let logout = FixMessage::logout(Some("Normal close"));
            self.send_message(logout).await?;
            
            // Update state
            self.transition_state(SessionState::LogoutSent).await;
            
            // In a real implementation, we'd wait for a logout response or timeout
            tokio::time::sleep(Duration::from_secs(2)).await;
            
            // Update state to disconnected
            self.transition_state(SessionState::Disconnected).await;
        }
        
        Ok(())
    }
    
    /// Get the current session state
    pub async fn get_state(&self) -> SessionState {
        self.state.lock().await.clone()
    }
}