use std::sync::Arc;
use tokio::time::Duration;
use async_trait::async_trait;

use fix_session_manager::config::SessionConfig;
use fix_session_manager::error::FixError;
use fix_session_manager::message::FixMessage;
use fix_session_manager::session::{SessionManager, SessionState, SessionCallback};

#[tokio::test]
async fn test_session_creation() {
    let mut config = SessionConfig::new("SENDER", "TARGET");
    config.begin_string = "FIX.4.4".to_string();
    config.heartbeat_interval = Duration::from_secs(30);
    
    let session_manager = SessionManager::new(config);
    
    let state = session_manager.get_state().await;
    assert_eq!(state, SessionState::Created);
}

struct TestCallback {
    messages: std::sync::Mutex<Vec<FixMessage>>,
    states: std::sync::Mutex<Vec<(SessionState, SessionState)>>,
    errors: std::sync::Mutex<Vec<FixError>>,
}

impl TestCallback {
    fn new() -> Self {
        Self {
            messages: std::sync::Mutex::new(Vec::new()),
            states: std::sync::Mutex::new(Vec::new()),
            errors: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SessionCallback for TestCallback {
    async fn on_message(&self, message: FixMessage) -> Result<(), FixError> {
        let mut messages = self.messages.lock().unwrap();
        messages.push(message);
        Ok(())
    }
    
    async fn on_state_change(&self, old_state: SessionState, new_state: SessionState) {
        let mut states = self.states.lock().unwrap();
        states.push((old_state, new_state));
    }
    
    async fn on_error(&self, error: FixError) {
        let mut errors = self.errors.lock().unwrap();
        errors.push(error);
    }
}

#[tokio::test]
async fn test_session_start() {
    let mut config = SessionConfig::new("SENDER", "TARGET");
    config.begin_string = "FIX.4.4".to_string();
    config.heartbeat_interval = Duration::from_secs(30);
    
    let mut session_manager = SessionManager::new(config);
    let callback = TestCallback::new();
    session_manager.set_callback(callback);
    
    // Start the session
    let result = session_manager.start().await;
    assert!(result.is_ok());
    
    // Check state change to LogonSent
    let state = session_manager.get_state().await;
    assert_eq!(state, SessionState::LogonSent);
}

// Helper function to simulate receiving a message
async fn simulate_receive_message(session_manager: &SessionManager, message: &[u8]) -> Result<(), FixError> {
    session_manager.handle_message(message).await
}

#[tokio::test]
async fn test_logon_sequence() {
    let mut config = SessionConfig::new("SENDER", "TARGET");
    config.begin_string = "FIX.4.4".to_string();
    config.heartbeat_interval = Duration::from_secs(30);
    
    let mut session_manager = SessionManager::new(config);
    let callback = TestCallback::new();
    session_manager.set_callback(callback);
    
    // Start the session - this will send a logon
    let result = session_manager.start().await;
    assert!(result.is_ok());
    
    // Simulate receiving a logon response
    let logon_response = b"8=FIX.4.4\x019=65\x0135=A\x0134=1\x0149=TARGET\x0156=SENDER\x0152=20230601-12:00:00\x0198=0\x01108=30\x0110=051\x01";
    let result = simulate_receive_message(&session_manager, logon_response).await;
    assert!(result.is_ok());
    
    // Wait a bit for async processing
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Check if state is Active
    let state = session_manager.get_state().await;
    assert_eq!(state, SessionState::Active);
    
    // Clean shutdown
    let result = session_manager.stop().await;
    assert!(result.is_ok());
    
    // Verify state is now Disconnected
    let state = session_manager.get_state().await;
    assert_eq!(state, SessionState::Disconnected);
}