pub const NEGOTIATION_VERSION: u16 = 1;

pub mod system {
    pub const CONNECTION_STATE_VERSION: u16 = 1;
    pub const PING_VERSION: u16 = 1;

    pub const CONNECTION_STATE_VERSIONS: &[u16] = &[CONNECTION_STATE_VERSION];
    pub const PING_VERSIONS: &[u16] = &[PING_VERSION];
}

pub mod auth {
    pub const AUTHENTICATE_VERSION: u16 = 1;
    pub const DISCOVERY_VERSION: u16 = 1;
    pub const SIGNUP_VERSION: u16 = 1;
    pub const TOKEN_VERSION: u16 = 1;

    pub const AUTHENTICATE_VERSIONS: &[u16] = &[AUTHENTICATE_VERSION];
    pub const DISCOVERY_VERSIONS: &[u16] = &[DISCOVERY_VERSION];
    pub const SIGNUP_VERSIONS: &[u16] = &[SIGNUP_VERSION];
    pub const TOKEN_VERSIONS: &[u16] = &[TOKEN_VERSION];
}

pub mod channels {
    pub const CREATE_VERSION: u16 = 1;
    pub const DELETE_VERSION: u16 = 1;
    pub const EVENTS_VERSION: u16 = 1;
    pub const READ_VERSION: u16 = 1;

    pub const CREATE_VERSIONS: &[u16] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[u16] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[u16] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[u16] = &[READ_VERSION];
}

pub mod memberships {
    pub const EVENTS_VERSION: u16 = 1;
    pub const READ_VERSION: u16 = 1;
    pub const WRITE_VERSION: u16 = 1;

    pub const EVENTS_VERSIONS: &[u16] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[u16] = &[READ_VERSION];
    pub const WRITE_VERSIONS: &[u16] = &[WRITE_VERSION];
}

pub mod messages {
    pub const CREATE_VERSION: u16 = 1;
    pub const DELETE_VERSION: u16 = 1;
    pub const EVENTS_VERSION: u16 = 1;
    pub const READ_VERSION: u16 = 1;

    pub const CREATE_VERSIONS: &[u16] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[u16] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[u16] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[u16] = &[READ_VERSION];
}

pub mod servers {
    pub const CREATE_VERSION: u16 = 1;
    pub const DELETE_VERSION: u16 = 1;
    pub const EVENTS_VERSION: u16 = 1;
    pub const READ_VERSION: u16 = 1;

    pub const CREATE_VERSIONS: &[u16] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[u16] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[u16] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[u16] = &[READ_VERSION];
}

pub mod users {
    pub const CREATE_VERSION: u16 = 1;
    pub const DELETE_VERSION: u16 = 1;
    pub const EVENTS_VERSION: u16 = 1;
    pub const READ_VERSION: u16 = 1;

    pub const CREATE_VERSIONS: &[u16] = &[CREATE_VERSION];
    pub const DELETE_VERSIONS: &[u16] = &[DELETE_VERSION];
    pub const EVENTS_VERSIONS: &[u16] = &[EVENTS_VERSION];
    pub const READ_VERSIONS: &[u16] = &[READ_VERSION];
}
