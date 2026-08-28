use std::collections::BTreeMap;

use crate::discord::ActivityInfo;

type ActivityKey = (String, i64);

#[derive(Default)]
pub(super) struct ActivityRegistry {
    // BTreeMap keeps broadcast order stable across changes.
    entries: BTreeMap<ActivityKey, ActivityInfo>,
}

impl ActivityRegistry {
    pub(super) fn set(&mut self, client_id: String, pid: i64, activity: ActivityInfo) {
        self.entries.insert((client_id, pid), activity);
    }

    pub(super) fn clear(&mut self, client_id: &str, pid: i64) {
        self.entries.remove(&(client_id.to_owned(), pid));
    }

    pub(super) fn activities(&self) -> Vec<ActivityInfo> {
        self.entries.values().cloned().collect()
    }

    pub(super) fn activity_for_client(&self, client_id: &str) -> Option<ActivityInfo> {
        self.entries
            .iter()
            .find(|((id, _pid), _)| id == client_id)
            .map(|(_, activity)| activity.clone())
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
        assert_eq!(names, ["Game A", "Song B"]);

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
}
