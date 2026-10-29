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
        deserialize_with = "description_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
}

fn description_patch<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        }
    }
}
