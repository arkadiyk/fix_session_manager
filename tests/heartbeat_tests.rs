use tokio::time::Duration;
use fix_session_manager::heartbeat::HeartbeatMonitor;

#[tokio::test]
async fn test_heartbeat_creation() {
    // Create a heartbeat monitor with 30-second interval
    let heartbeat = HeartbeatMonitor::new(Duration::from_secs(30));
    
    // Check initial state
    assert_eq!(heartbeat.heartbeat_interval(), Duration::from_secs(30));
}

#[tokio::test]
async fn test_heartbeat_tracking() {
    // Create a heartbeat monitor with a short interval for testing
    let heartbeat = HeartbeatMonitor::new(Duration::from_millis(100));
    
    // Should not need to send heartbeat initially
    assert!(!heartbeat.should_send_heartbeat().await);
    
    // Mark message as sent
    heartbeat.message_sent().await;
    
    // Wait for heartbeat interval to pass
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    // Now should need to send heartbeat
    assert!(heartbeat.should_send_heartbeat().await);
    
    // After sending another message, should reset the timer
    heartbeat.message_sent().await;
    assert!(!heartbeat.should_send_heartbeat().await);
}

#[tokio::test]
async fn test_test_request() {
    // Create a heartbeat monitor with a very short timeout for testing
    let heartbeat = HeartbeatMonitor::new(Duration::from_millis(100));
    
    // Mark message as received
    heartbeat.message_received().await;
    
    // Wait until test request threshold (3 heartbeat intervals = 300ms)
    tokio::time::sleep(Duration::from_millis(350)).await;
    
    // Should need to send test request
    assert!(heartbeat.should_send_test_request().await);
    
    // Generate test request ID
    let test_req_id = heartbeat.generate_test_request_id().await;
    assert!(!test_req_id.is_empty());
    
    // No timeout yet
    assert!(!heartbeat.check_test_request_timeout().await);
    
    // Wait for test request timeout (1.5 * heartbeat interval = 150ms)
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Should timeout
    assert!(heartbeat.check_test_request_timeout().await);
    
    // Acknowledge the test request
    let result = heartbeat.acknowledge_test_request(&test_req_id).await;
    assert!(result.is_ok());
    
    // After acknowledge, should not timeout
    assert!(!heartbeat.check_test_request_timeout().await);
}