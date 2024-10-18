use super::types::StreamVariant;

/// Returns a StreamVariant::ServerHint that contains some information about the server.
/// Is intended to be sent as a heartbeat to the client.
pub fn heartbeat_content() -> StreamVariant {
    // As a temporary impementation, just write CPU: 0 in the JSON.

    let mut heartbeat_json = serde_json::Map::new();
    heartbeat_json.insert(
        "CPU".to_string(),
        serde_json::Value::Number(serde_json::Number::from(0)),
    );

    let heartbeat_string = serde_json::Value::Object(heartbeat_json).to_string();

    StreamVariant::ServerHint(heartbeat_string)
}
