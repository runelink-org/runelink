use runelink_client::validation::{validate_host, validate_username};
use runelink_types::{Host, Server, ServerMembership, UserRef};
use std::{collections::HashMap, fmt::Display};

use crate::error::CliError;

/// Returns the prefix for a list item given an optional active value
pub fn get_prefix<T: PartialEq>(
    val: T,
    active: Option<T>,
    len: usize,
) -> &'static str {
    if len == 1 {
        return "";
    }
    let Some(active) = active else {
        return "";
    };
    if val == active { "* " } else { "  " }
}

pub fn group_memberships_by_host<'a>(
    memberships: &'a Vec<ServerMembership>,
) -> HashMap<&'a str, Vec<&'a ServerMembership>> {
    let mut map = HashMap::<&'a str, Vec<&'a ServerMembership>>::new();
    for membership in memberships {
        let host = membership.server.host.as_str();
        map.entry(host).or_default().push(membership);
    }
    map
}

pub fn parse_username_input(
    input: &str,
    strict: bool,
) -> Result<String, CliError> {
    let normalized = validate_username(input)
        .map_err(|error| CliError::InvalidArgument(error.to_string()))?;
    if strict && normalized != input {
        return Err(CliError::InvalidArgument(format!(
            "Username must already be normalized in strict mode: `{normalized}`"
        )));
    }
    Ok(normalized)
}

pub fn parse_host_input(input: &str, strict: bool) -> Result<String, CliError> {
    let normalized = validate_host(input)
        .map_err(|error| CliError::InvalidArgument(error.to_string()))?;
    if strict && normalized != input {
        return Err(CliError::InvalidArgument(format!(
            "Host must already be normalized in strict mode: `{normalized}`"
        )));
    }
    Ok(normalized)
}

pub fn parse_optional_host_input(
    input: Option<&str>,
    strict: bool,
) -> Result<Option<Host>, CliError> {
    input
        .map(|value| {
            Host::parse(&parse_host_input(value, strict)?)
                .map_err(|error| CliError::InvalidArgument(error.to_string()))
        })
        .transpose()
}

pub fn parse_user_ref_input(
    name: &str,
    host: &str,
    strict: bool,
) -> Result<UserRef, CliError> {
    UserRef::new(
        parse_username_input(name, strict)?,
        parse_host_input(host, strict)?,
    )
    .map_err(|error| CliError::InvalidArgument(error.to_string()))
}

/// Format an optional full ID consistently across CLI lists.
pub fn list_label(
    label: impl Display,
    id: impl Display,
    show_ids: bool,
) -> String {
    if show_ids {
        format!("{label} [{id}]")
    } else {
        label.to_string()
    }
}

/// Format a server for lists and prompts, with its optional full ID and
/// host.
pub fn server_label(
    server: &Server,
    show_ids: bool,
    show_hosts: bool,
) -> String {
    match (show_ids, show_hosts) {
        (true, true) => {
            format!("{server} [{} / {}]", server.host, server.id)
        }
        (true, false) => format!("{server} [{}]", server.id),
        (false, true) => format!("{server} [{}]", server.host),
        (false, false) => server.to_string(),
    }
}
