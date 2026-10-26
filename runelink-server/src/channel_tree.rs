//! Validation and ordering rules for a server's channel tree.

use std::collections::HashMap;

use runelink_types::{
    channel::{ServerChannel, ServerChannelId, ServerChannelMove},
    server::ServerId,
};

use crate::error::{ApiError, ApiResult};

/// Check that `parent` may contain channels of `server_id`.
pub fn check_parent(
    server_id: ServerId,
    parent: &ServerChannel,
) -> ApiResult<()> {
    if parent.server_id != server_id {
        return Err(ApiError::BadRequest(
            "Parent channel not found in specified server".into(),
        ));
    }
    if !parent.is_category() {
        return Err(ApiError::BadRequest(
            "Parent channel must be a category".into(),
        ));
    }
    Ok(())
}

/// Plan moving a channel and its subtree within `channels`, which must be
/// all of one server's channels.
///
/// Returns the channels whose parent or position changes, with the new
/// values applied. The target siblings are renumbered from zero.
pub fn plan_move(
    channels: &[ServerChannel],
    channel_id: ServerChannelId,
    channel_move: &ServerChannelMove,
) -> ApiResult<Vec<ServerChannel>> {
    let channel = channels
        .iter()
        .find(|c| c.id == channel_id)
        .ok_or(ApiError::NotFound)?;
    let parent_id = channel_move.parent_channel_id;
    if let Some(parent_id) = parent_id {
        let parent =
            channels.iter().find(|c| c.id == parent_id).ok_or_else(|| {
                ApiError::BadRequest(
                    "Parent channel not found in specified server".into(),
                )
            })?;
        check_parent(channel.server_id, parent)?;
        if is_self_or_descendant(channels, parent_id, channel_id) {
            return Err(ApiError::BadRequest(
                "A channel can not be moved into itself or its descendants"
                    .into(),
            ));
        }
    }

    let mut siblings: Vec<&ServerChannel> = channels
        .iter()
        .filter(|c| c.parent_channel_id == parent_id && c.id != channel_id)
        .collect();
    siblings.sort_by_key(|c| (c.position, c.id.as_uuid()));
    let index = (channel_move.index as usize).min(siblings.len());
    siblings.insert(index, channel);

    let changed = siblings
        .into_iter()
        .zip(0..)
        .filter(|(c, position)| {
            c.position != *position || c.parent_channel_id != parent_id
        })
        .map(|(c, position)| ServerChannel {
            parent_channel_id: parent_id,
            position,
            ..c.clone()
        })
        .collect();
    Ok(changed)
}

/// Whether `id` is `ancestor_id` or lies beneath it.
fn is_self_or_descendant(
    channels: &[ServerChannel],
    id: ServerChannelId,
    ancestor_id: ServerChannelId,
) -> bool {
    let parents: HashMap<_, _> = channels
        .iter()
        .map(|c| (c.id, c.parent_channel_id))
        .collect();
    let mut current = Some(id);
    // Bounded in case stored data already contains a cycle
    for _ in 0..=channels.len() {
        match current {
            Some(id) if id == ancestor_id => return true,
            Some(id) => current = parents.get(&id).copied().flatten(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use runelink_types::channel::{ServerChannelContent, ServerChannelKind};
    use time::OffsetDateTime;
    use uuid::Uuid;

    fn server_id() -> ServerId {
        Uuid::from_u128(1).into()
    }

    fn id(n: u128) -> ServerChannelId {
        Uuid::from_u128(n).into()
    }

    fn channel(
        n: u128,
        kind: ServerChannelKind,
        parent: Option<u128>,
        position: i32,
    ) -> ServerChannel {
        ServerChannel {
            id: id(n),
            server_id: server_id(),
            content: match kind {
                ServerChannelKind::Text => ServerChannelContent::Text {
                    text_channel_id: Uuid::from_u128(1000 + n).into(),
                },
                ServerChannelKind::Category => ServerChannelContent::Category,
            },
            title: format!("channel-{n}"),
            description: None,
            parent_channel_id: parent.map(id),
            position,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn text(n: u128, parent: Option<u128>, position: i32) -> ServerChannel {
        channel(n, ServerChannelKind::Text, parent, position)
    }

    fn category(n: u128, parent: Option<u128>, position: i32) -> ServerChannel {
        channel(n, ServerChannelKind::Category, parent, position)
    }

    fn to(parent: Option<u128>, index: u32) -> ServerChannelMove {
        ServerChannelMove {
            parent_channel_id: parent.map(id),
            index,
        }
    }

    /// Changed channels as `(id, parent, position)`.
    fn placements(
        changed: Vec<ServerChannel>,
    ) -> Vec<(u128, Option<u128>, i32)> {
        changed
            .into_iter()
            .map(|c| {
                (
                    c.id.as_uuid().as_u128(),
                    c.parent_channel_id.map(|p| p.as_uuid().as_u128()),
                    c.position,
                )
            })
            .collect()
    }

    fn bad_request(result: ApiResult<Vec<ServerChannel>>) -> String {
        match result {
            Err(ApiError::BadRequest(message)) => message,
            other => panic!("expected bad request, got {other:?}"),
        }
    }

    #[test]
    fn reorders_siblings_within_a_parent() {
        let channels =
            [text(10, None, 0), text(11, None, 1), text(12, None, 2)];
        let changed = plan_move(&channels, id(12), &to(None, 0)).unwrap();
        assert_eq!(
            placements(changed),
            [(12, None, 0), (10, None, 1), (11, None, 2)]
        );
        let changed = plan_move(&channels, id(10), &to(None, 1)).unwrap();
        assert_eq!(placements(changed), [(11, None, 0), (10, None, 1)]);
    }

    #[test]
    fn unchanged_placement_is_a_no_op() {
        let channels = [text(10, None, 0), text(11, None, 1)];
        assert!(
            plan_move(&channels, id(11), &to(None, 1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn index_past_the_end_places_last() {
        let channels = [text(10, None, 0), text(11, None, 1)];
        let changed = plan_move(&channels, id(10), &to(None, 99)).unwrap();
        assert_eq!(placements(changed), [(11, None, 0), (10, None, 1)]);
    }

    #[test]
    fn duplicate_positions_order_by_id() {
        let channels =
            [text(12, None, 0), text(11, None, 0), text(10, None, 0)];
        // Ties resolve as 10, 11, 12; channel 11 already has position 0
        let changed = plan_move(&channels, id(10), &to(None, 2)).unwrap();
        assert_eq!(placements(changed), [(12, None, 1), (10, None, 2)]);
    }

    #[test]
    fn moves_a_subtree_between_parents() {
        let channels = [
            category(1, None, 0),
            category(2, None, 1),
            text(10, Some(1), 0),
            category(3, Some(1), 1),
            text(11, Some(3), 0),
            text(12, Some(2), 0),
        ];
        // Category 3 keeps its child; only the moved row and new siblings change
        let changed = plan_move(&channels, id(3), &to(Some(2), 0)).unwrap();
        assert_eq!(placements(changed), [(3, Some(2), 0), (12, Some(2), 1)]);
        let changed = plan_move(&channels, id(11), &to(None, 1)).unwrap();
        assert_eq!(placements(changed), [(11, None, 1), (2, None, 2)]);
    }

    #[test]
    fn nests_categories_without_a_depth_limit() {
        let mut channels: Vec<_> = (1..=50)
            .map(|n| category(n, (n > 1).then(|| n - 1), 0))
            .collect();
        channels.push(text(100, None, 1));
        let changed = plan_move(&channels, id(100), &to(Some(50), 0)).unwrap();
        assert_eq!(placements(changed), [(100, Some(50), 0)]);
    }

    #[test]
    fn rejects_cycles() {
        let channels = [
            category(1, None, 0),
            category(2, Some(1), 0),
            category(3, Some(2), 0),
        ];
        let message = bad_request(plan_move(&channels, id(1), &to(Some(1), 0)));
        assert!(message.contains("itself or its descendants"));
        let message = bad_request(plan_move(&channels, id(1), &to(Some(3), 0)));
        assert!(message.contains("itself or its descendants"));
        assert!(plan_move(&channels, id(3), &to(Some(1), 0)).is_ok());
    }

    #[test]
    fn rejects_non_category_parents() {
        let channels = [text(10, None, 0), text(11, None, 1)];
        let message =
            bad_request(plan_move(&channels, id(11), &to(Some(10), 0)));
        assert!(message.contains("must be a category"));
    }

    #[test]
    fn rejects_parents_outside_the_server() {
        let channels = [text(10, None, 0)];
        let message =
            bad_request(plan_move(&channels, id(10), &to(Some(9), 0)));
        assert!(message.contains("not found"));

        let mut other = category(9, None, 0);
        other.server_id = Uuid::from_u128(2).into();
        let message = match check_parent(server_id(), &other) {
            Err(ApiError::BadRequest(message)) => message,
            other => panic!("expected bad request, got {other:?}"),
        };
        assert!(message.contains("not found"));
    }

    #[test]
    fn unknown_channel_is_not_found() {
        let channels = [text(10, None, 0)];
        assert!(matches!(
            plan_move(&channels, id(11), &to(None, 0)),
            Err(ApiError::NotFound)
        ));
    }
}
