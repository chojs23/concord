use crate::discord::ids::{Id, marker::UserMarker};
use crate::discord::{ActivityInfo, PresenceStatus};

use super::model::ChannelPaneEntry;
use super::{DashboardState, primary_compact_activity};

/// One visual line in the channel pane.
///
/// Activity rows keep their parent's entry index so navigation can resolve the
/// line without giving the activity an independent cursor.
#[derive(Clone, Debug)]
pub(in crate::tui) enum ChannelPaneRow<'a> {
    Entry {
        entry_index: usize,
        entry: ChannelPaneEntry<'a>,
    },
    Activity {
        owner_entry_index: usize,
        entry: ChannelPaneEntry<'a>,
        recipient_id: Id<UserMarker>,
        activity: &'a ActivityInfo,
    },
}

impl<'a> ChannelPaneRow<'a> {
    pub(in crate::tui) fn entry_index(&self) -> usize {
        match self {
            Self::Entry { entry_index, .. } => *entry_index,
            Self::Activity {
                owner_entry_index, ..
            } => *owner_entry_index,
        }
    }

    pub(in crate::tui) fn entry(&self) -> &ChannelPaneEntry<'a> {
        match self {
            Self::Entry { entry, .. } | Self::Activity { entry, .. } => entry,
        }
    }

    pub(in crate::tui) fn activity(&self) -> Option<&'a ActivityInfo> {
        match self {
            Self::Activity { activity, .. } => Some(activity),
            Self::Entry { .. } => None,
        }
    }

    pub(in crate::tui) fn is_entry(&self) -> bool {
        matches!(self, Self::Entry { .. })
    }
}

impl DashboardState {
    pub(in crate::tui) fn channel_pane_rows(&self) -> Vec<ChannelPaneRow<'_>> {
        let entries = self.channel_pane_filtered_entries();
        self.channel_pane_rows_from_entries(&entries)
    }

    pub(in crate::tui) fn channel_pane_rows_from_entries<'a>(
        &'a self,
        entries: &[ChannelPaneEntry<'a>],
    ) -> Vec<ChannelPaneRow<'a>> {
        let mut rows = Vec::with_capacity(entries.len().saturating_mul(2));

        for (entry_index, entry) in entries.iter().enumerate() {
            rows.push(ChannelPaneRow::Entry {
                entry_index,
                entry: entry.clone(),
            });

            let ChannelPaneEntry::Channel { state: channel, .. } = entry else {
                continue;
            };
            if !channel.is_dm() {
                continue;
            }
            let Some(recipient) = channel.recipients.first() else {
                continue;
            };
            if matches!(
                recipient.status,
                PresenceStatus::Offline | PresenceStatus::Unknown
            ) {
                continue;
            }
            let Some(activity) = primary_compact_activity(self.user_activities(recipient.user_id))
            else {
                continue;
            };

            rows.push(ChannelPaneRow::Activity {
                owner_entry_index: entry_index,
                entry: entry.clone(),
                recipient_id: recipient.user_id,
                activity,
            });
        }

        rows
    }

    pub(in crate::tui) fn visible_channel_pane_rows(&self) -> Vec<ChannelPaneRow<'_>> {
        self.channel_pane_rows()
            .into_iter()
            .skip(self.channel_scroll())
            .take(self.channel_content_height())
            .collect()
    }
}
