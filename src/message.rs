use bytes::{BufMut, BytesMut};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use crate::error::FixError;

/// Standard FIX message field separator (SOH character)
pub const FIELD_SEPARATOR: u8 = 0x01;

/// Common FIX message tags
pub mod tags {
    pub const BEGIN_STRING: u32 = 8;
    pub const BODY_LENGTH: u32 = 9;
    pub const MSG_TYPE: u32 = 35;
    pub const SENDER_COMP_ID: u32 = 49;
    pub const TARGET_COMP_ID: u32 = 56;
    pub const MSG_SEQ_NUM: u32 = 34;
    pub const SENDING_TIME: u32 = 52;
    pub const CHECK_SUM: u32 = 10;
    pub const TEXT: u32 = 58;
    pub const GAP_FILL_FLAG: u32 = 123;
    pub const NEW_SEQ_NO: u32 = 36;
    pub const HEARTBEAT_INT: u32 = 108;
    pub const TEST_REQ_ID: u32 = 112;
    pub const BEGIN_SEQ_NO: u32 = 7;
    pub const END_SEQ_NO: u32 = 16;
    pub const RESET_SEQ_NUM_FLAG: u32 = 141; // Added this here
}

/// FIX message types
pub mod message_types {
    pub const HEARTBEAT: &str = "0";
    pub const TEST_REQUEST: &str = "1";
    pub const RESEND_REQUEST: &str = "2";
    pub const REJECT: &str = "3";
    pub const SEQUENCE_RESET: &str = "4";
    pub const LOGOUT: &str = "5";
    pub const LOGON: &str = "A";
}

/// Represents a parsed FIX message
#[derive(Debug, Clone)]
pub struct FixMessage {
    fields: HashMap<u32, String>,
    msg_type: String,
}

impl FixMessage {
    /// Create a new empty FIX message
    pub fn new(msg_type: &str) -> Self {
        let mut fields = HashMap::new();
        fields.insert(tags::MSG_TYPE, msg_type.to_string());
        
        Self {
            fields,
            msg_type: msg_type.to_string(),
        }
    }
    
    /// Set a field value
    pub fn set_field<T: ToString>(&mut self, tag: u32, value: T) -> &mut Self {
        self.fields.insert(tag, value.to_string());
        self
    }
    
    /// Get a field value
    pub fn get_field(&self, tag: u32) -> Option<&String> {
        self.fields.get(&tag)
    }
    
    /// Get the message type
    pub fn msg_type(&self) -> &str {
        &self.msg_type
    }
    
    /// Get the message sequence number
    pub fn seq_num(&self) -> Result<u64, FixError> {
        self.get_field(tags::MSG_SEQ_NUM)
            .ok_or_else(|| FixError::InvalidFormat("Missing sequence number".to_string()))
            .and_then(|s| s.parse::<u64>().map_err(|_| FixError::ParseError("Invalid sequence number".to_string())))
    }
    
    /// Convert the message to a byte array for transmission
    pub fn to_bytes(&self, begin_string: &str, sender_comp_id: &str, target_comp_id: &str, seq_num: u64) -> BytesMut {
        let mut body = BytesMut::new();
        
        add_field(&mut body, tags::MSG_TYPE, &self.msg_type);
        
        if !self.fields.contains_key(&tags::MSG_SEQ_NUM) {
            add_field(&mut body, tags::MSG_SEQ_NUM, &seq_num.to_string());
        }
        
        if !self.fields.contains_key(&tags::SENDER_COMP_ID) {
            add_field(&mut body, tags::SENDER_COMP_ID, sender_comp_id);
        }
        
        if !self.fields.contains_key(&tags::TARGET_COMP_ID) {
            add_field(&mut body, tags::TARGET_COMP_ID, target_comp_id);
        }
        
        if !self.fields.contains_key(&tags::SENDING_TIME) {
            let now: DateTime<Utc> = Utc::now();
            add_field(&mut body, tags::SENDING_TIME, &now.format("%Y%m%d-%H:%M:%S.%3f").to_string());
        }
        
        for (tag, value) in &self.fields {
            if *tag == tags::MSG_TYPE || *tag == tags::MSG_SEQ_NUM || 
               *tag == tags::SENDER_COMP_ID || *tag == tags::TARGET_COMP_ID || 
               *tag == tags::SENDING_TIME {
                continue;
            }
            
            add_field(&mut body, *tag, value);
        }
        
        let mut message = BytesMut::new();
        add_field(&mut message, tags::BEGIN_STRING, begin_string);
        add_field(&mut message, tags::BODY_LENGTH, &body.len().to_string());
        message.put(body);
        
        let checksum: u32 = message.iter().map(|&b| u32::from(b)).sum::<u32>() % 256;
        add_field(&mut message, tags::CHECK_SUM, &format!("{:03}", checksum));
        
        message
    }
    
    /// Parse a FIX message from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, FixError> {
        let mut fields = HashMap::new();
        let data_str = std::str::from_utf8(data)
            .map_err(|_| FixError::InvalidFormat("Invalid UTF-8 data".to_string()))?;
        
        let field_pairs = data_str.split(char::from(FIELD_SEPARATOR));
        let mut msg_type = String::new();
        
        for pair in field_pairs {
            if pair.is_empty() {
                continue;
            }
            
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() != 2 {
                continue;
            }
            
            let tag = parts[0].parse::<u32>()
                .map_err(|_| FixError::InvalidFormat(format!("Invalid tag: {}", parts[0])))?;
            
            let value = parts[1].to_string();
            
            if tag == tags::MSG_TYPE {
                msg_type = value.clone();
            }
            
            fields.insert(tag, value);
        }
        
        if msg_type.is_empty() {
            return Err(FixError::InvalidFormat("Missing message type".to_string()));
        }
        
        Ok(Self { fields, msg_type })
    }
    
    /// Create a Logon message
    pub fn logon(heartbeat_interval: u64, reset_seq_num: bool) -> Self {
        let mut message = Self::new(message_types::LOGON);
        message.set_field(tags::HEARTBEAT_INT, heartbeat_interval);
        
        if reset_seq_num {
            message.set_field(tags::RESET_SEQ_NUM_FLAG, "Y");
        }
        
        message
    }
    
    /// Create a Heartbeat message, optionally in response to a TestRequest
    pub fn heartbeat(test_req_id: Option<&str>) -> Self {
        let mut message = Self::new(message_types::HEARTBEAT);
        
        if let Some(id) = test_req_id {
            message.set_field(tags::TEST_REQ_ID, id);
        }
        
        message
    }
    
    /// Create a TestRequest message
    pub fn test_request(id: &str) -> Self {
        let mut message = Self::new(message_types::TEST_REQUEST);
        message.set_field(tags::TEST_REQ_ID, id);
        
        message
    }
    
    /// Create a ResendRequest message
    pub fn resend_request(begin_seq_no: u64, end_seq_no: u64) -> Self {
        let mut message = Self::new(message_types::RESEND_REQUEST);
        message.set_field(tags::BEGIN_SEQ_NO, begin_seq_no);
        message.set_field(tags::END_SEQ_NO, end_seq_no);
        
        message
    }
    
    /// Create a SequenceReset message
    pub fn sequence_reset(new_seq_no: u64, gap_fill: bool) -> Self {
        let mut message = Self::new(message_types::SEQUENCE_RESET);
        message.set_field(tags::NEW_SEQ_NO, new_seq_no);
        
        if gap_fill {
            message.set_field(tags::GAP_FILL_FLAG, "Y");
        }
        
        message
    }
    
    /// Create a Logout message
    pub fn logout(reason: Option<&str>) -> Self {
        let mut message = Self::new(message_types::LOGOUT);
        
        if let Some(text) = reason {
            message.set_field(tags::TEXT, text);
        }
        
        message
    }
}

// Helper function to add a field to a message buffer
fn add_field<T: AsRef<str>>(buffer: &mut BytesMut, tag: u32, value: T) {
    buffer.put_slice(tag.to_string().as_bytes());
    buffer.put_u8(b'=');
    buffer.put_slice(value.as_ref().as_bytes());
    buffer.put_u8(FIELD_SEPARATOR);
}