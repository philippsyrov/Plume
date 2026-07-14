//! Ollama streaming request serialization, including optional image inputs.

use base64::Engine as _;

use super::super::{ChatMessage, ChatRole};
use super::http::role_str;

pub(super) fn build_request_body_streaming_with_images(
    model: &str,
    messages: &[ChatMessage],
    images: &[Vec<u8>],
) -> String {
    let final_user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User);
    let messages_json = serde_json::Value::Array(
        messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                let mut value = serde_json::json!({
                    "role": role_str(message.role),
                    "content": message.content,
                });
                if Some(index) == final_user && !images.is_empty() {
                    value["images"] = serde_json::Value::Array(
                        images
                            .iter()
                            .map(|image| {
                                serde_json::Value::String(
                                    base64::engine::general_purpose::STANDARD.encode(image),
                                )
                            })
                            .collect(),
                    );
                }
                value
            })
            .collect(),
    );
    serde_json::json!({
        "model": model,
        "messages": messages_json,
        "stream": true,
    })
    .to_string()
}
