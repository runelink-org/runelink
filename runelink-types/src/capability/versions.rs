pub const NEGOTIATION_VERSION: u16 = 1;

pub mod system {
    use crate::capability::CapabilityVersion;

    pub const CONNECTION_STATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const PING_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);

    pub const CONNECTION_STATE_VERSIONS: &[CapabilityVersion] =
        &[CONNECTION_STATE_VERSION];
    pub const PING_VERSIONS: &[CapabilityVersion] = &[PING_VERSION];
}

pub mod auth {
    use crate::capability::CapabilityVersion;

    pub const AUTHENTICATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const DISCOVERY_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const SIGNUP_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const TOKEN_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);

    pub const AUTHENTICATE_VERSIONS: &[CapabilityVersion] =
        &[AUTHENTICATE_VERSION];
    pub const DISCOVERY_VERSIONS: &[CapabilityVersion] = &[DISCOVERY_VERSION];
    pub const SIGNUP_VERSIONS: &[CapabilityVersion] = &[SIGNUP_VERSION];
    pub const TOKEN_VERSIONS: &[CapabilityVersion] = &[TOKEN_VERSION];
}

pub mod channels {
    use crate::capability::CapabilityVersion;

    pub const CREATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);
    pub const DELETE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);
    pub const EVENTS_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);
    pub const MOVE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);
    pub const READ_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);

    pub const CREATE_VERSIONS: &[CapabilityVersion] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[CapabilityVersion] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[CapabilityVersion] = &[EVENTS_VERSION];
    pub const MOVE_VERSIONS: &[CapabilityVersion] = &[MOVE_VERSION];
    pub const READ_VERSIONS: &[CapabilityVersion] = &[READ_VERSION];
}

pub mod memberships {
    use crate::capability::CapabilityVersion;

    pub const EVENTS_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const READ_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const WRITE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);

    pub const EVENTS_VERSIONS: &[CapabilityVersion] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[CapabilityVersion] = &[READ_VERSION];
    pub const WRITE_VERSIONS: &[CapabilityVersion] = &[WRITE_VERSION];
}

pub mod messages {
    use crate::capability::CapabilityVersion;

    pub const CREATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 19);
    pub const DELETE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 19);
    pub const EVENTS_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 19);
    pub const READ_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 19);

    pub const CREATE_VERSIONS: &[CapabilityVersion] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[CapabilityVersion] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[CapabilityVersion] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[CapabilityVersion] = &[READ_VERSION];
}

pub mod servers {
    use crate::capability::CapabilityVersion;

    pub const CREATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const DELETE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const EVENTS_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const READ_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 17);

    pub const CREATE_VERSIONS: &[CapabilityVersion] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[CapabilityVersion] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[CapabilityVersion] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[CapabilityVersion] = &[READ_VERSION];
}

pub mod users {
    use crate::capability::CapabilityVersion;

    pub const CREATE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const DELETE_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const EVENTS_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);
    pub const READ_VERSION: CapabilityVersion =
        CapabilityVersion::new(2026, 10, 16);

    pub const CREATE_VERSIONS: &[CapabilityVersion] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[CapabilityVersion] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[CapabilityVersion] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[CapabilityVersion] = &[READ_VERSION];
}
