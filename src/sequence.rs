use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use std::collections::VecDeque;
use crate::error::FixError;
use std::time::{Instant, Duration};

/// Manages sequence numbers for a FIX session
pub struct SequenceManager {
    // Current outbound sequence number
    outbound_seqnum: AtomicU64,
    
    // Expected next inbound sequence number
    inbound_seqnum: AtomicU64,
    
    // Store of pending resend requests (begin_seq, end_seq, time_requested)
    pending_resends: Mutex<Vec<(u64, u64, Instant)>>,
    
    // Buffer for out-of-sequence messages
    message_buffer: Mutex<VecDeque<(u64, Vec<u8>)>>,
    
    // Maximum size for the message buffer
    buffer_max_size: usize,
    
    // Resend request timeout
    resend_timeout: Duration,
}

impl SequenceManager {
    /// Create a new sequence manager with the specified sequence numbers
    pub fn new(initial_outbound: u64, initial_inbound: u64) -> Self {
        Self {
            outbound_seqnum: AtomicU64::new(initial_outbound),
            inbound_seqnum: AtomicU64::new(initial_inbound),
            pending_resends: Mutex::new(Vec::new()),
            message_buffer: Mutex::new(VecDeque::new()),
            buffer_max_size: 10000, // Default buffer size
            resend_timeout: Duration::from_secs(30), // Default timeout
        }
    }
    
    /// Get the next outbound sequence number and increment
    pub fn next_outbound_seqnum(&self) -> u64 {
        self.outbound_seqnum.fetch_add(1, Ordering::SeqCst)
    }
    
    /// Get the current outbound sequence number without incrementing
    pub fn current_outbound_seqnum(&self) -> u64 {
        self.outbound_seqnum.load(Ordering::SeqCst)
    }
    
    /// Get the expected inbound sequence number
    pub fn expected_inbound_seqnum(&self) -> u64 {
        self.inbound_seqnum.load(Ordering::SeqCst)
    }
    
    /// Reset sequence numbers
    pub fn reset(&self, outbound: u64, inbound: u64) {
        self.outbound_seqnum.store(outbound, Ordering::SeqCst);
        self.inbound_seqnum.store(inbound, Ordering::SeqCst);
        
        // Instead of spawning a task, we'll create a separate method to clear the state
        // that can be called by the user of this API
    }
    
    /// Clear pending resends and message buffer 
    pub async fn clear_state(&self) {
        let mut pending = self.pending_resends.lock().await;
        pending.clear();
        
        let mut buffer = self.message_buffer.lock().await;
        buffer.clear();
    }
    
    /// Process an inbound sequence number
    pub async fn process_inbound_seqnum(&self, received_seqnum: u64) -> Result<bool, FixError> {
        let expected = self.inbound_seqnum.load(Ordering::SeqCst);
        
        if received_seqnum < expected {
            // This is a duplicate or old message
            return Err(FixError::SequenceGap { 
                expected, 
                received: received_seqnum 
            });
        } else if received_seqnum > expected {
            // Gap detected, trigger recovery
            return Ok(false);
        }
        
        // Sequence number is as expected
        self.inbound_seqnum.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }
    
    /// Add a message to the buffer for later processing
    pub async fn buffer_message(&self, seqnum: u64, message: Vec<u8>) -> Result<(), FixError> {
        let mut buffer = self.message_buffer.lock().await;
        
        // Check buffer size limit
        if buffer.len() >= self.buffer_max_size {
            return Err(FixError::ProtocolViolation(
                "Message buffer overflow".to_string()
            ));
        }
        
        // Insert message in order
        let pos = buffer.iter().position(|(seq, _)| *seq > seqnum);
        match pos {
            Some(idx) => buffer.insert(idx, (seqnum, message)),
            None => buffer.push_back((seqnum, message)),
        }
        
        Ok(())
    }
    
    /// Register a pending resend request
    pub async fn request_resend(&self, begin_seq: u64, end_seq: u64) -> Result<(), FixError> {
        let mut pending_resends = self.pending_resends.lock().await;
        pending_resends.push((begin_seq, end_seq, Instant::now()));
        Ok(())
    }
    
    /// Check if resend requests have timed out
    pub async fn check_resend_timeouts(&self) -> Vec<(u64, u64)> {
        let mut pending_resends = self.pending_resends.lock().await;
        let now = Instant::now();
        let mut timed_out = Vec::new();
        
        // Keep only non-timed-out requests
        pending_resends.retain(|(begin, end, time)| {
            let elapsed = now.duration_since(*time);
            if elapsed > self.resend_timeout {
                timed_out.push((*begin, *end));
                false
            } else {
                true
            }
        });
        
        timed_out
    }
    
    /// Process buffered messages that are in sequence
    pub async fn process_buffered_messages(&self) -> Vec<Vec<u8>> {
        let mut buffer = self.message_buffer.lock().await;
        let mut messages_to_process = Vec::new();
        let expected = self.inbound_seqnum.load(Ordering::SeqCst);
        
        // Find consecutive messages starting from expected
        while !buffer.is_empty() {
            if let Some((seq, _)) = buffer.front() {
                if *seq != expected + messages_to_process.len() as u64 {
                    break;
                }
                
                if let Some((_, msg)) = buffer.pop_front() {
                    messages_to_process.push(msg);
                }
            } else {
                break;
            }
        }
        
        // Update inbound seqnum if we processed any messages
        if !messages_to_process.is_empty() {
            self.inbound_seqnum.fetch_add(messages_to_process.len() as u64, Ordering::SeqCst);
        }
        
        messages_to_process
    }
}