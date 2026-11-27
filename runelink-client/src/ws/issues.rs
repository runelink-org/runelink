use runelink_types::capability::{
    Capability, CapabilityVersion, Incompatibility, incompatibility_reason,
};

/// An offered capability the host did not agree to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityIssue {
    pub capability: Capability,
    pub offered_versions: Vec<CapabilityVersion>,
    /// The versions the host supports, or `None` for older hosts that do not
    /// say. An empty list means the capability is missing.
    pub supported_versions: Option<Vec<CapabilityVersion>>,
    /// Why the versions have nothing in common, or `None` when the host did
    /// not say which versions it supports.
    pub reason: Option<Incompatibility>,
    pub message: String,
}

impl CapabilityIssue {
    pub(super) fn new(
        capability: Capability,
        offered_versions: Vec<CapabilityVersion>,
        supported_versions: Option<Vec<CapabilityVersion>>,
    ) -> Self {
        let (reason, message) = match supported_versions.as_deref() {
            Some(supported) => {
                let reason =
                    incompatibility_reason(&offered_versions, supported);
                let message = format!(
                    "{capability}: {reason} (client offers {}; host supports {})",
                    list(&offered_versions),
                    list(supported),
                );
                (Some(reason), message)
            }
            None => (
                None,
                format!("{capability}: compatibility details unavailable"),
            ),
        };
        Self {
            capability,
            offered_versions,
            supported_versions,
            reason,
            message,
        }
    }
}

fn list(versions: &[CapabilityVersion]) -> String {
    if versions.is_empty() {
        return "none".into();
    }
    versions
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
