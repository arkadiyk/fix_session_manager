use fix_session_manager::message::{FixMessage, message_types, tags};
use fix_session_manager::error::FixError;

#[test]
fn test_message_creation() {
    // Create a logon message
    let logon = FixMessage::logon(30, false);
    
    // Validate message type
    assert_eq!(logon.msg_type(), message_types::LOGON);
    
    // Validate heartbeat interval field
    assert_eq!(logon.get_field(tags::HEARTBEAT_INT).map(|s| s.as_str()), Some("30"));
    
    // Validate reset flag (should be absent)
    assert_eq!(logon.get_field(tags::RESET_SEQ_NUM_FLAG), None);
    
    // Create a logon message with reset flag
    let logon_with_reset = FixMessage::logon(30, true);
    assert_eq!(logon_with_reset.get_field(tags::RESET_SEQ_NUM_FLAG).map(|s| s.as_str()), Some("Y"));
    
    // Create a logout message
    let logout = FixMessage::logout(Some("Testing logout"));
    
    // Validate message type
    assert_eq!(logout.msg_type(), message_types::LOGOUT);
    
    // Validate text field
    assert_eq!(logout.get_field(tags::TEXT).map(|s| s.as_str()), Some("Testing logout"));
    
    // Create a heartbeat message
    let heartbeat = FixMessage::heartbeat(Some("TEST123"));
    
    // Validate message type
    assert_eq!(heartbeat.msg_type(), message_types::HEARTBEAT);
    
    // Validate test req id field
    assert_eq!(heartbeat.get_field(tags::TEST_REQ_ID).map(|s| s.as_str()), Some("TEST123"));
}

#[test]
fn test_message_parsing() {
    // Raw FIX message bytes
    let raw_msg = b"8=FIX.4.4\x019=65\x0135=A\x0134=1\x0149=SENDER\x0156=TARGET\x0152=20230601-12:00:00\x01108=30\x0110=123\x01";
    
    // Parse message
    let result = FixMessage::from_bytes(raw_msg);
    assert!(result.is_ok());
    
    let message = result.unwrap();
    
    // Validate message fields
    assert_eq!(message.msg_type(), message_types::LOGON);
    assert_eq!(message.get_field(tags::MSG_SEQ_NUM).map(|s| s.as_str()), Some("1"));
    assert_eq!(message.get_field(tags::SENDER_COMP_ID).map(|s| s.as_str()), Some("SENDER"));
    assert_eq!(message.get_field(tags::TARGET_COMP_ID).map(|s| s.as_str()), Some("TARGET"));
    assert_eq!(message.get_field(tags::HEARTBEAT_INT).map(|s| s.as_str()), Some("30"));
    
    // Convert back to bytes
    let bytes = message.to_bytes("FIX.4.4", "SENDER", "TARGET", 1);
    
    // Parse again to verify
    let reparsed = FixMessage::from_bytes(&bytes);
    assert!(reparsed.is_ok());
}

#[test]
fn test_message_invalid_format() {
    // Invalid message (missing separator)
    let invalid_msg = b"8=FIX.4.4\x019=65\x0135A\x0134=1\x0149=SENDER\x0156=TARGET\x0110=123\x01";
    
    // Parse should fail
    let result = FixMessage::from_bytes(invalid_msg);
    assert!(result.is_err());
    
    match result {
        Err(FixError::InvalidFormat(_)) => (), // Expected error
        _ => panic!("Expected InvalidFormat error"),
    }
    
    // Another invalid message (missing required tag)
    let invalid_msg = b"8=FIX.4.4\x019=40\x0135=A\x0149=SENDER\x0156=TARGET\x0110=123\x01";
    
    // Parse should succeed but seq_num() should fail
    let result = FixMessage::from_bytes(invalid_msg);
    assert!(result.is_ok());
    
    let msg = result.unwrap();
    let seq_result = msg.seq_num();
    assert!(seq_result.is_err());
}

#[test]
fn test_message_fields() {
    // Create a new empty message
    let mut message = FixMessage::new(message_types::TEST_REQUEST);
    
    // Add some fields
    message.set_field(tags::SENDING_TIME, "20230601-12:00:00");
    message.set_field(tags::MSG_SEQ_NUM, "100");
    message.set_field(tags::TEST_REQ_ID, "TEST123");
    
    // Check fields
    assert_eq!(message.get_field(tags::SENDING_TIME).map(|s| s.as_str()), Some("20230601-12:00:00"));
    assert_eq!(message.get_field(tags::MSG_SEQ_NUM).map(|s| s.as_str()), Some("100"));
    assert_eq!(message.get_field(tags::TEST_REQ_ID).map(|s| s.as_str()), Some("TEST123"));
    
    // Update a field
    message.set_field(tags::TEST_REQ_ID, "TEST456");
    assert_eq!(message.get_field(tags::TEST_REQ_ID).map(|s| s.as_str()), Some("TEST456"));
    
    // Check msg_type remains
    assert_eq!(message.msg_type(), message_types::TEST_REQUEST);
}

#[test]
fn test_checksum_calculation() {
    // Create a message with known content
    let mut message = FixMessage::new(message_types::TEST_REQUEST);
    message.set_field(tags::TEST_REQ_ID, "TEST");
    
    // Convert to bytes with proper FIX format
    let bytes = message.to_bytes("FIX.4.4", "SENDER", "TARGET", 1);
    
    // The bytes should end with a checksum field
    let bytes_str = std::str::from_utf8(&bytes).unwrap();
    assert!(bytes_str.contains("10="));
    
    // Parse back to verify checksum validation
    let result = FixMessage::from_bytes(&bytes);
    assert!(result.is_ok());
}