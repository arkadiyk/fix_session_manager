use std::sync::Arc;
use tokio::time::Duration;

use fix_session_manager::sequence::SequenceManager;
use fix_session_manager::error::FixError;

#[tokio::test]
async fn test_sequence_management() {
    // Create a new sequence manager with initial values
    let seq_manager = SequenceManager::new(1, 1);
    
    // Check initial values
    assert_eq!(seq_manager.current_outbound_seqnum(), 1);
    assert_eq!(seq_manager.expected_inbound_seqnum(), 1);
    
    // Get next outbound sequence number (increments it)
    let next_seq = seq_manager.next_outbound_seqnum();
    assert_eq!(next_seq, 1);
    assert_eq!(seq_manager.current_outbound_seqnum(), 2);
    
    // Process a valid inbound sequence number
    let result = seq_manager.process_inbound_seqnum(1).await;
    assert!(result.is_ok());
    assert!(result.unwrap()); // Indicates sequence is good
    assert_eq!(seq_manager.expected_inbound_seqnum(), 2);
    
    // Process another valid inbound sequence number
    let result = seq_manager.process_inbound_seqnum(2).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(seq_manager.expected_inbound_seqnum(), 3);
    
    // Process a duplicate sequence number (should be rejected)
    let result = seq_manager.process_inbound_seqnum(2).await;
    assert!(result.is_ok());
    assert!(!result.unwrap()); // Should return false for duplicate
    assert_eq!(seq_manager.expected_inbound_seqnum(), 3); // Remains unchanged
    
    // Process a sequence with a gap
    let result = seq_manager.process_inbound_seqnum(5).await;
    assert!(result.is_ok());
    assert!(!result.unwrap()); // Should return false for gap
    assert_eq!(seq_manager.expected_inbound_seqnum(), 3); // Remains unchanged
}

#[tokio::test]
async fn test_sequence_reset() {
    // Create a new sequence manager
    let seq_manager = SequenceManager::new(5, 10);
    
    // Reset the sequences
    seq_manager.reset(100, 200);
    
    // Check the new values
    assert_eq!(seq_manager.current_outbound_seqnum(), 100);
    assert_eq!(seq_manager.expected_inbound_seqnum(), 200);
}

#[tokio::test]
async fn test_message_buffering() {
    // Create a new sequence manager
    let seq_manager = SequenceManager::new(1, 1);
    
    // Buffer some out-of-sequence messages
    let msg1 = b"8=FIX.4.4\x019=50\x0135=D\x0134=3\x0149=SENDER\x0156=TARGET\x0110=123\x01".to_vec();
    let msg2 = b"8=FIX.4.4\x019=50\x0135=D\x0134=2\x0149=SENDER\x0156=TARGET\x0110=122\x01".to_vec();
    
    // Buffer the messages (out of order)
    let result = seq_manager.buffer_message(3, msg1).await;
    assert!(result.is_ok());
    
    let result = seq_manager.buffer_message(2, msg2).await;
    assert!(result.is_ok());
    
    // Process the first expected sequence (1)
    let result = seq_manager.process_inbound_seqnum(1).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
    
    // Process the buffered messages
    let buffered_msgs = seq_manager.process_buffered_messages().await;
    assert_eq!(buffered_msgs.len(), 2);
    
    // Expected sequence should now be 4
    assert_eq!(seq_manager.expected_inbound_seqnum(), 4);
}