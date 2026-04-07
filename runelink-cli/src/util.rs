use runelink_client::validation::{validate_host, validate_username};
use runelink_types::{ServerMembership, UserRef};
use std::collections::HashMap;

use crate::error::CliError;

/// Returns the prefix for a list item given an optional default value
pub fn get_prefix<T: PartialEq>(
    val: T,
    default: Option<T>,
    len: usize,
) -> &'static str {
    if len == 1 {
        return "";
    }
    let Some(default) = default else {
        return "";
    };
    if val == default { "* " } else { "  " }
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
) -> Result<Option<String>, CliError> {
    input
        .map(|value| parse_host_input(value, strict))
        .transpose()
}

pub fn parse_user_ref_input(
    name: &str,
    host: &str,
    strict: bool,
) -> Result<UserRef, CliError> {
    Ok(UserRef::new(
        parse_username_input(name, strict)?,
        parse_host_input(host, strict)?,
    ))
}
