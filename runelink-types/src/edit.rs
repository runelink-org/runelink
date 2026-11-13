use serde::{Deserialize, Deserializer, Serialize};
use time::OffsetDateTime;

/// Changes to server or channel presentation. Omitted fields are unchanged;
/// an explicit null description clears it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DetailsUpdate {
    #[serde(with = "time::serde::rfc3339")]
    pub expected_updated_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
}

pub(crate) fn nullable_patch<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        channel::{NewServerChannel, ServerChannel, ServerChannelUpdate},
        server::{NewServer, Server},
    };
    use serde_json::json;

    #[test]
    fn resources_and_creation_omit_absent_descriptions() {
        for description in [None, Some(json!(null)), Some(json!("Details"))] {
            let mut wire = json!({
                "id":"00000000-0000-4000-8000-000000000001",
                "server_id":"00000000-0000-4000-8000-000000000002",
                "host":"localhost", "title":"test", "kind":"category",
                "position":0, "parent_channel_id":null,
                "created_at":"2026-11-13T00:00:00Z", "updated_at":"2026-11-13T00:00:00Z"
            });
            if let Some(description) = &description {
                wire["description"] = description.clone();
            }
            let server: Server = serde_json::from_value(wire.clone()).unwrap();
            let new_server: NewServer =
                serde_json::from_value(wire.clone()).unwrap();
            let channel: ServerChannel =
                serde_json::from_value(wire.clone()).unwrap();
            let new_channel: NewServerChannel =
                serde_json::from_value(wire).unwrap();
            for resource in [
                serde_json::to_value(server).unwrap(),
                serde_json::to_value(new_server).unwrap(),
                serde_json::to_value(channel).unwrap(),
                serde_json::to_value(new_channel).unwrap(),
            ] {
                if description
                    .as_ref()
                    .is_some_and(serde_json::Value::is_string)
                {
                    assert_eq!(
                        resource.get("description"),
                        description.as_ref()
                    );
                } else {
                    assert!(resource.get("description").is_none());
                }
            }
        }
    }
    #[test]
    fn description_patch_preserves_missing_null_and_value() {
        for value in [
            json!({}),
            json!({"description": null}),
            json!({"description": "new"}),
        ] {
            let mut wire = value;
            wire["expected_updated_at"] = json!("2026-10-28T00:00:00Z");
            let update: DetailsUpdate =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(update).unwrap(), wire);
            let update: ServerChannelUpdate =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(update).unwrap(), wire);
        }
    }
}
