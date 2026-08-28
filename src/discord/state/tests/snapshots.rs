use std::sync::Arc;

use super::*;
use crate::discord::state::{SnapshotAreas, SnapshotRevision};

#[test]
fn snapshots_share_caches_and_writes_detach_only_the_touched_areas() {
    let mut state = DiscordState::default();
    let channel_id = Id::new(1);
    let base = SnapshotRevision::default().advance(SnapshotAreas::all());
    let snapshot = state.snapshot(base);

    // Taking a snapshot must not copy anything.
    assert!(Arc::ptr_eq(
        &state.navigation,
        &snapshot.navigation.navigation
    ));
    assert!(Arc::ptr_eq(
        &state.message_cache,
        &snapshot.message.message_cache
    ));
    assert!(Arc::ptr_eq(
        &state.notifications,
        &snapshot.detail.notifications
    ));

    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(10),
        ..MessageCreateFixture::test_fixture_default()
    }));

    // The message cache the write lands in detaches from the snapshot, while
    // areas a message arrival has no business touching stay shared.
    assert!(!Arc::ptr_eq(
        &state.message_cache,
        &snapshot.message.message_cache
    ));
    assert!(Arc::ptr_eq(
        &state.guild_details,
        &snapshot.navigation.guild_details
    ));
    assert!(Arc::ptr_eq(&state.voice, &snapshot.navigation.voice));
    assert!(Arc::ptr_eq(&state.session, &snapshot.navigation.session));

    // Restoring reattaches the moved area by pointer rather than copying it.
    let next = base.advance(SnapshotAreas::message());
    let mut reader = snapshot.to_state();
    reader.restore_snapshot_areas(&state.snapshot(next), base);

    assert!(Arc::ptr_eq(&reader.message_cache, &state.message_cache));
    assert_eq!(reader.messages_for_channel(channel_id).len(), 1);
}
