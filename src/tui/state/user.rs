use std::collections::HashSet;

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{AppCommand, MessageState, PresenceStatus, UserProfileInfo};

use super::{ActiveGuildScope, DashboardState};
use super::{
    member_grouping::{
        MemberEntry, MemberGroup, channel_recipient_group, flatten_member_groups,
        guild_member_groups,
    },
    model::{FocusPane, GuildPaneEntry, MemberActionItem, MemberActionKind},
    popups::{MemberActionMenuState, UserProfilePopupState},
    scroll::{clamp_selected_index, move_index_down, move_index_up},
};

impl DashboardState {
    pub fn is_user_profile_popup_open(&self) -> bool {
        self.user_profile_popup.is_some()
    }

    pub fn is_member_action_menu_open(&self) -> bool {
        self.member_action_menu.is_some()
    }

    pub fn member_action_menu_title(&self) -> Option<String> {
        let menu = self.member_action_menu.as_ref()?;
        let entries = self.flattened_members();
        let entry = entries.iter().find(|m| m.user_id() == menu.user_id)?;
        Some(entry.display_name())
    }

    /// Direct shortcut from the member pane: open the profile popup for the
    /// currently selected member without going through the action menu.
    pub fn show_selected_member_profile(&mut self) -> Option<AppCommand> {
        if self.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_selected_member_actions(&mut self) {
        if self.focus != FocusPane::Members {
            return;
        }
        let entries = self.flattened_members();
        let Some(entry) = entries.get(self.selected_member()) else {
            return;
        };
        let user_id = entry.user_id();
        // For DM/group-DM panes there is no guild context; pass it through so
        // the profile fetch can omit `guild_id` and skip the guild_member
        // section gracefully.
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.member_action_menu = Some(MemberActionMenuState {
            user_id,
            guild_id,
            selected: 0,
        });
    }

    pub fn close_member_action_menu(&mut self) {
        self.member_action_menu = None;
    }

    pub fn selected_member_action_items(&self) -> Vec<MemberActionItem> {
        if self.member_action_menu.is_none() {
            return Vec::new();
        }
        vec![MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }]
    }

    pub fn selected_member_action_index(&self) -> Option<usize> {
        let menu = self.member_action_menu.as_ref()?;
        Some(clamp_selected_index(
            menu.selected,
            self.selected_member_action_items().len(),
        ))
    }

    pub fn move_member_action_down(&mut self) {
        let len = self.selected_member_action_items().len();
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_down(&mut menu.selected, len);
        }
    }

    pub fn move_member_action_up(&mut self) {
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_up(&mut menu.selected);
        }
    }

    pub fn select_member_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_member_action_items().len() {
            return false;
        }
        if let Some(menu) = self.member_action_menu.as_mut() {
            menu.selected = row;
            return true;
        }
        false
    }

    pub fn activate_selected_member_action(&mut self) -> Option<AppCommand> {
        let menu = self.member_action_menu.clone()?;
        let items = self.selected_member_action_items();
        let item = items
            .get(clamp_selected_index(menu.selected, items.len()))?
            .clone();
        if !item.enabled {
            return None;
        }
        match item.kind {
            MemberActionKind::ShowProfile => {
                self.close_member_action_menu();
                self.open_user_profile_popup(menu.user_id, menu.guild_id)
            }
        }
    }

    /// Opens the profile popup for `user_id`. Returns
    /// `AppCommand::LoadUserProfile` to fetch fresh data when nothing is
    /// cached yet — the popup itself shows a loading state in the meantime.
    pub fn open_user_profile_popup(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        self.user_profile_popup = Some(UserProfilePopupState {
            user_id,
            guild_id,
            load_error: None,
            mutual_cursor: None,
        });
        if self.discord.user_profile(user_id, guild_id).is_some() {
            None
        } else {
            Some(AppCommand::LoadUserProfile { user_id, guild_id })
        }
    }

    pub fn close_user_profile_popup(&mut self) {
        self.user_profile_popup = None;
    }

    pub fn user_profile_popup_data(&self) -> Option<&UserProfileInfo> {
        let popup = self.user_profile_popup.as_ref()?;
        self.discord.user_profile(popup.user_id, popup.guild_id)
    }

    pub fn user_profile_popup_load_error(&self) -> Option<&str> {
        self.user_profile_popup
            .as_ref()
            .and_then(|popup| popup.load_error.as_deref())
    }

    pub fn user_profile_popup_status(&self) -> PresenceStatus {
        let Some(popup) = self.user_profile_popup.as_ref() else {
            return PresenceStatus::Unknown;
        };

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .members_for_guild(guild_id)
                .into_iter()
                .find(|member| member.user_id == popup.user_id)
                .map(|member| member.status)
        {
            return status;
        }

        let recipient_status = self
            .discord
            .channels_for_guild(None)
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == popup.user_id)
            .map(|recipient| recipient.status);

        recipient_status
            .filter(|status| *status != PresenceStatus::Unknown)
            .or_else(|| self.discord.user_presence(popup.user_id))
            .unwrap_or(PresenceStatus::Unknown)
    }

    /// URL of the avatar image to render into the open profile popup. None
    /// when the popup is closed, the profile has not loaded yet, or the user
    /// has no avatar attachment.
    pub fn user_profile_popup_avatar_url(&self) -> Option<&str> {
        self.user_profile_popup_data()?.avatar_url.as_deref()
    }

    /// Index of the currently highlighted mutual-server line, if the user has
    /// moved into the list with j/k. None while the cursor sits idle.
    pub fn user_profile_popup_mutual_cursor(&self) -> Option<usize> {
        let popup = self.user_profile_popup.as_ref()?;
        let cursor = popup.mutual_cursor?;
        let len = self.user_profile_popup_data()?.mutual_guilds.len();
        if len == 0 {
            None
        } else {
            Some(cursor.min(len - 1))
        }
    }

    fn user_profile_popup_mutual_len(&self) -> usize {
        self.user_profile_popup_data()
            .map(|profile| profile.mutual_guilds.len())
            .unwrap_or(0)
    }

    pub fn move_user_profile_popup_down(&mut self) {
        let len = self.user_profile_popup_mutual_len();
        if len == 0 {
            return;
        }
        if let Some(popup) = self.user_profile_popup.as_mut() {
            let next = popup.mutual_cursor.map(|c| c + 1).unwrap_or(0);
            popup.mutual_cursor = Some(next.min(len - 1));
        }
    }

    pub fn move_user_profile_popup_up(&mut self) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.mutual_cursor = match popup.mutual_cursor {
                Some(0) | None => Some(0),
                Some(c) => Some(c - 1),
            };
        }
    }

    /// Resolves a one-on-one DM channel with the user the popup is showing,
    /// switching the dashboard to it. If a DM is already in state, opens it
    /// immediately. Otherwise emits an `OpenDirectMessage` command so the
    /// REST handler can create or fetch the channel and (via
    /// `AppEvent::ActivateChannel`) bring the UI to it once it lands.
    pub fn open_direct_message_with_profile_target(&mut self) -> Option<AppCommand> {
        let user_id = self.user_profile_popup.as_ref()?.user_id;
        if Some(user_id) == self.current_user_id {
            // Discord doesn't expose self-DMs; quietly do nothing rather
            // than firing a REST call we know will fail.
            return None;
        }
        if let Some(channel_id) = self.find_one_on_one_dm_channel(user_id) {
            self.close_user_profile_popup();
            self.activate_guild(ActiveGuildScope::DirectMessages);
            self.activate_channel(channel_id);
            return None;
        }
        self.close_user_profile_popup();
        Some(AppCommand::OpenDirectMessage { user_id })
    }

    fn find_one_on_one_dm_channel(
        &self,
        user_id: Id<UserMarker>,
    ) -> Option<crate::discord::ids::Id<crate::discord::ids::marker::ChannelMarker>> {
        self.discord
            .channels_for_guild(None)
            .into_iter()
            .find(|channel| {
                channel.recipients.len() == 1
                    && channel
                        .recipients
                        .iter()
                        .any(|recipient| recipient.user_id == user_id)
            })
            .map(|channel| channel.id)
    }

    /// Activates the mutual server highlighted in the popup and closes the
    /// popup. Returns None when no cursor is set or the data isn't loaded
    /// yet — Enter then falls through to a no-op (the caller can still rely
    /// on Esc to close).
    pub fn activate_selected_user_profile_mutual(&mut self) -> Option<AppCommand> {
        let cursor = self.user_profile_popup_mutual_cursor()?;
        let guild_id = self
            .user_profile_popup_data()?
            .mutual_guilds
            .get(cursor)?
            .guild_id;
        // Bail out if we don't actually know the guild yet (rare; the popup
        // can list mutual_guilds whose GUILD_CREATE hasn't been delivered for
        // this session).
        self.discord.guild(guild_id)?;
        self.activate_guild(ActiveGuildScope::Guild(guild_id));
        if let Some(index) = self.guild_pane_entries().iter().position(
            |entry| matches!(entry, GuildPaneEntry::Guild { state, .. } if state.id == guild_id),
        ) {
            self.selected_guild = index;
        }
        self.close_user_profile_popup();
        None
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let members = self.discord.members_for_guild(guild_id);
        let roles = self.discord.roles_for_guild(guild_id);
        guild_member_groups(members, roles)
    }

    pub fn message_author_role_color(&self, message: &MessageState) -> Option<u32> {
        let channel = self.discord.channel(message.channel_id);
        let guild_id = message
            .guild_id
            .or_else(|| channel.and_then(|channel| channel.guild_id));
        let guild_id = guild_id?;
        self.discord.message_author_role_color(
            guild_id,
            message.channel_id,
            message.id,
            message.author_id,
        )
    }

    pub fn missing_message_author_profile_requests(
        &self,
    ) -> Vec<(Id<UserMarker>, Id<GuildMarker>)> {
        let mut seen = HashSet::new();
        let mut requests = Vec::new();

        for message in self.visible_messages() {
            let guild_id = message.guild_id.or_else(|| {
                self.discord
                    .channel(message.channel_id)
                    .and_then(|channel| channel.guild_id)
            });
            self.push_missing_author_profile_request(
                &mut requests,
                &mut seen,
                message.author_id,
                guild_id,
            );
        }

        for post in self.visible_forum_post_items() {
            let guild_id = self
                .discord
                .channel(post.channel_id)
                .and_then(|channel| channel.guild_id);
            if let Some(author_id) = post.preview_author_id {
                self.push_missing_author_profile_request(
                    &mut requests,
                    &mut seen,
                    author_id,
                    guild_id,
                );
            }
        }

        requests
    }

    fn push_missing_author_profile_request(
        &self,
        requests: &mut Vec<(Id<UserMarker>, Id<GuildMarker>)>,
        seen: &mut HashSet<(Id<UserMarker>, Id<GuildMarker>)>,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        let Some(guild_id) = guild_id else {
            return;
        };
        if self
            .discord
            .member_display_name(guild_id, user_id)
            .is_some()
            || self.discord.user_profile(user_id, Some(guild_id)).is_some()
            || !seen.insert((user_id, guild_id))
        {
            return;
        }
        requests.push((user_id, guild_id));
    }

    pub fn member_role_color(&self, member: MemberEntry<'_>) -> Option<u32> {
        let guild_id = self.selected_guild_id()?;
        self.discord.member_role_color(guild_id, member.user_id())
    }

    pub fn member_panel_title(&self) -> String {
        let Some(guild_id) = self.selected_guild_id() else {
            return "Members".to_owned();
        };
        let Some(member_count) = self
            .discord
            .guild(guild_id)
            .and_then(|guild| guild.member_count)
        else {
            return "Members".to_owned();
        };

        let loaded = self.discord.members_for_guild(guild_id).len();
        format!("Members {loaded}/{member_count} loaded")
    }

    fn selected_channel_recipient_group(&self) -> Vec<MemberGroup<'_>> {
        let Some(channel) = self.selected_channel_state() else {
            return Vec::new();
        };
        channel_recipient_group(channel)
    }

    pub fn flattened_members(&self) -> Vec<MemberEntry<'_>> {
        flatten_member_groups(self.members_grouped())
    }
}
