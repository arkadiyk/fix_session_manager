# Fix Session Manager
 
* **Sequence Number Management:** Tracking incoming and outgoing messages, detecting gaps, and requesting resends.
* **Heartbeat & Connection Monitoring:** Maintaining connectivity and quickly detecting issues.
* **Error Handling & Recovery:** Efficiently managing out-of-sequence or missing messages to ensure the session remains synchronized.
* **Low Latency:** Being highly performant to meet the real-time demands of trading operations.


## Sequence Number Management
The `sequence.rs` module handles tracking incoming and outgoing message sequence numbers, detecting gaps, and requesting resends when necessary. 

It uses atomic operations for thread safety and includes a buffer for out-of-sequence messages.

## Heartbeat & Connection Monitoring

The `heartbeat.rs` module implements connection monitoring through heartbeats and test requests, quickly detecting connectivity issues through configurable timeouts.

## Error Handling & Recovery 

The `recovery.rs` module provides sophisticated error recovery, handling out-of-sequence messages and various error conditions to maintain session synchronization.

## Low Latency Design

The implementation is built with performance in mind, using:

* `Tokio` for asynchronous processing
* `Atomic` operations where appropriate
* Efficient message buffering
* Minimal locking patterns
* Optimized byte handling with the bytes crate

The session manager is structured to handle the FIX protocol lifecycle efficiently, including:

* Session establishment with logon/logout sequences
* Message sequencing and gap detection
* Heartbeat monitoring and test requests
* Message buffering and recovery for out-of-sequence messages
* Clean error handling using the thiserror crate

The main SessionManager class provides a high-level API with methods to start/stop sessions and send/receive messages, while internally managing all the complex protocol details.

To use this session manager in a real application, you would:

1. Create a `SessionConfig` with your session parameters
1. Instantiate a `SessionManager` with this config
1. Implement the `SessionCallback` trait to receive messages and state changes
1. Connect the `msg_receiver` to your network transport layer
1. Start the session with `session_manager.start()`
