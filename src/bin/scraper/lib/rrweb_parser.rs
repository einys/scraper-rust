use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct RrwebEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

pub fn parse_rrweb(data: &str) -> Vec<RrwebEvent> {
    serde_json::from_str(data).unwrap_or_default()
}
