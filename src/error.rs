use thiserror::Error;
use std::io;

#[derive(Error, Debug)]
pub enum FixError {
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    
    #[error("Invalid message format: {0}")]
    InvalidFormat(String),
    
    #[error("Sequence gap detected: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    
    #[error("Session rejected: {0}")]
    SessionRejected(String),
    
    #[error("Heartbeat timeout")]
    HeartbeatTimeout,
    
    #[error("Connection lost: {0}")]
    ConnectionLost(String),
    
    #[error("Message parsing error: {0}")]
    ParseError(String),
    
    #[error("Timeout: {0}")]
    Timeout(String),
    
    #[error("Protocol violation: {0}")]
    ProtocolViolation(String),
}