use std::fmt;

use uuid::Uuid;

#[repr(transparent)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct ConnId(Uuid);

impl ConnId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for ConnId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ConnId> for Uuid {
    fn from(value: ConnId) -> Self {
        value.0
    }
}

impl fmt::Debug for ConnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
