use runelink_types::ServerMembership;
use std::collections::HashMap;

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
    if val == default {
        "* "
    } else {
        "  "
    }
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
