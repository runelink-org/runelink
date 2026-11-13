/// Application-defined JSON object. Core does not interpret its keys or merge
/// channel metadata into messages.
pub type Metadata = serde_json::Map<String, serde_json::Value>;

/// Maximum encoded size of a metadata object, in bytes.
pub const MAX_METADATA_BYTES: usize = 64 * 1024;

pub fn validate_metadata(metadata: &Metadata) -> Result<(), String> {
    if serde_json::to_vec(metadata)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_METADATA_BYTES
    {
        return Err(format!(
            "Metadata must be at most {MAX_METADATA_BYTES} bytes"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        channel::{NewServerChannel, ServerChannelUpdate},
        message::{MessageUpdate, NewMessage},
    };
    use serde_json::{Value, json};

    #[test]
    fn creation_normalizes_null_and_missing_to_omission() {
        for metadata in [
            None,
            Some(Value::Null),
            Some(json!({"nested": {"enabled": true}, "items": [1, null]})),
        ] {
            let mut message = json!({"author": {"name":"test", "host":"localhost"}, "body":"hello"});
            let mut channel = json!({"kind":"text", "title":"test", "description":null, "parent_channel_id":null});
            if let Some(value) = metadata.clone() {
                message["metadata"] = value.clone();
                channel["metadata"] = value;
            }
            let message: NewMessage = serde_json::from_value(message).unwrap();
            let channel: NewServerChannel =
                serde_json::from_value(channel).unwrap();
            for wire in [
                serde_json::to_value(message).unwrap(),
                serde_json::to_value(channel).unwrap(),
            ] {
                if metadata.as_ref().is_some_and(Value::is_object) {
                    assert_eq!(wire.get("metadata"), metadata.as_ref());
                } else {
                    assert!(wire.get("metadata").is_none());
                }
            }
        }
    }

    #[test]
    fn edits_round_trip_omitted_null_and_object_metadata() {
        for fields in [
            json!({}),
            json!({"metadata":null}),
            json!({"metadata":{}}),
            json!({"metadata":{"kind":"result"}}),
        ] {
            let mut wire = fields;
            wire["expected_updated_at"] = json!("2026-11-13T00:00:00Z");
            let message: MessageUpdate =
                serde_json::from_value(wire.clone()).unwrap();
            let channel: ServerChannelUpdate =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(message).unwrap(), wire);
            assert_eq!(serde_json::to_value(channel).unwrap(), wire);
        }
    }

    #[test]
    fn metadata_requires_an_object_and_has_a_size_limit() {
        for value in [json!([]), json!("text"), json!(false), json!(1)] {
            let wire = json!({"metadata":value,"expected_updated_at":"2026-11-13T00:00:00Z"});
            assert!(
                serde_json::from_value::<MessageUpdate>(wire.clone()).is_err()
            );
            assert!(
                serde_json::from_value::<ServerChannelUpdate>(wire).is_err()
            );
        }
        let mut metadata = Metadata::new();
        metadata.insert("value".into(), json!("x".repeat(MAX_METADATA_BYTES)));
        assert!(validate_metadata(&metadata).is_err());
        assert!(validate_metadata(&Metadata::new()).is_ok());
    }
}
