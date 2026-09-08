use std::collections::BTreeMap;

use crate::discord::ActivityInfo;

type ActivityKey = (String, i64);

/// Monotonic counter stamping each `set` so the most recently updated entry
/// (the one the native client would display) can be found later.
#[derive(Default)]
pub(super) struct ActivityRegistry {
    // BTreeMap keeps broadcast order stable across changes.
    entries: BTreeMap<ActivityKey, (ActivityInfo, u64)>,
    next_sequence: u64,
}

impl ActivityRegistry {
    pub(super) fn set(&mut self, client_id: String, pid: i64, activity: ActivityInfo) {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.saturating_add(1);
        self.entries.insert((client_id, pid), (activity, sequence));
    }

    pub(super) fn clear(&mut self, client_id: &str, pid: i64) {
        self.entries.remove(&(client_id.to_owned(), pid));
    }

    /// All activities, most recently updated first. Also drives the UI picker
    /// order, so the top entry is what automatic mode would broadcast.
    pub(super) fn activities(&self) -> Vec<ActivityInfo> {
        let mut entries: Vec<(ActivityInfo, u64)> = self.entries.values().cloned().collect();
        entries.sort_by(|(_, a), (_, b)| b.cmp(a));
        entries.into_iter().map(|(activity, _)| activity).collect()
    }

    pub(super) fn activity_for_client(&self, client_id: &str) -> Option<ActivityInfo> {
        self.entries
            .iter()
            .find(|((id, _pid), _)| id == client_id)
            .map(|(_, (activity, _))| activity.clone())
    }

    /// The most recently updated activity across all connected apps, matching
    /// the native client's last-writer-wins behavior.
    pub(super) fn latest_activity(&self) -> Option<ActivityInfo> {
        self.entries
            .iter()
            .max_by_key(|(_, (_, sequence))| *sequence)
            .map(|(_, (activity, _))| activity.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::ActivityRegistry;
    use crate::discord::{ActivityInfo, ActivityKind};

    #[test]
    fn registry_aggregates_and_clears_per_app() {
        let mut registry = ActivityRegistry::default();
        registry.set(
            "app-a".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Game A"),
        );
        registry.set(
            "app-b".to_owned(),
            2,
            ActivityInfo::test(ActivityKind::Listening, "Song B"),
        );

        let activities = registry.activities();
        let names: Vec<&str> = activities
            .iter()
            .map(|activity| activity.name.as_str())
            .collect();
        // Most recently updated first.
        assert_eq!(names, ["Song B", "Game A"]);

        registry.set(
            "app-a".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Game A2"),
        );
        registry.clear("app-b", 2);
        let activities = registry.activities();
        let names: Vec<&str> = activities
            .iter()
            .map(|activity| activity.name.as_str())
            .collect();
        assert_eq!(names, ["Game A2"]);
    }

    #[test]
    fn activity_for_client_returns_latest_by_client_id() {
        let mut registry = ActivityRegistry::default();
        registry.set(
            "vscode".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Editing a.rs"),
        );
        assert_eq!(
            registry
                .activity_for_client("vscode")
                .map(|activity| activity.name),
            Some("Editing a.rs".to_owned())
        );

        registry.set(
            "vscode".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Editing b.rs"),
        );
        assert_eq!(
            registry
                .activity_for_client("vscode")
                .map(|activity| activity.name),
            Some("Editing b.rs".to_owned())
        );

        assert!(registry.activity_for_client("unknown").is_none());
    }

    #[test]
    fn latest_activity_returns_most_recent_update() {
        let mut registry = ActivityRegistry::default();
        registry.set(
            "vscode".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Editing a.rs"),
        );
        assert_eq!(
            registry.latest_activity().map(|activity| activity.name),
            Some("Editing a.rs".to_owned())
        );

        registry.set(
            "game".to_owned(),
            2,
            ActivityInfo::test(ActivityKind::Playing, "Epic Game"),
        );
        assert_eq!(
            registry.latest_activity().map(|activity| activity.name),
            Some("Epic Game".to_owned())
        );

        // Re-updating an older entry makes it the most recent again.
        registry.set(
            "vscode".to_owned(),
            1,
            ActivityInfo::test(ActivityKind::Playing, "Editing b.rs"),
        );
        assert_eq!(
            registry.latest_activity().map(|activity| activity.name),
            Some("Editing b.rs".to_owned())
        );

        // Clearing the latest entry falls back to the next most recent.
        registry.clear("vscode", 1);
        assert_eq!(
            registry.latest_activity().map(|activity| activity.name),
            Some("Epic Game".to_owned())
        );

        registry.clear("game", 2);
        assert!(registry.latest_activity().is_none());
    }
}
