use chrono::{DateTime, TimeZone, Utc};

use crate::discord::{
    GuildVerificationLevel,
    ids::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
    state::{ChannelState, DiscordState, GuildMemberState},
};

const DISCORD_EPOCH_MILLIS: i64 = 1_420_070_400_000;
const ACCOUNT_AGE_SECONDS: i64 = 5 * 60;
const MEMBER_AGE_SECONDS: i64 = 10 * 60;
const MEMBER_FLAG_BYPASSES_VERIFICATION: u64 = 1 << 2;

/// The first local verification rule that prevents a message send.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MessageVerificationRestriction {
    MembershipScreening,
    EmailVerificationRequired,
    AccountTooNew { remaining_seconds: u64 },
    MemberTooNew { remaining_seconds: u64 },
    PhoneVerificationRequired,
    VerificationDataUnavailable,
    UnsupportedLevel { value: u64 },
}

impl DiscordState {
    /// Return the server verification rule that currently prevents a send.
    pub fn message_verification_restriction(
        &self,
        channel: &ChannelState,
    ) -> Option<MessageVerificationRestriction> {
        self.message_verification_restriction_at(channel, Utc::now())
    }

    pub(crate) fn message_verification_restriction_at(
        &self,
        channel: &ChannelState,
        now: DateTime<Utc>,
    ) -> Option<MessageVerificationRestriction> {
        let guild_id = channel.guild_id?;
        let guild = self.guild(guild_id)?;
        let level = guild.verification_level;
        if let GuildVerificationLevel::Unknown(value) = level {
            return Some(MessageVerificationRestriction::UnsupportedLevel { value });
        }
        let Some(current_user_id) = self.current_user_id() else {
            return (!matches!(level, GuildVerificationLevel::None))
                .then_some(MessageVerificationRestriction::VerificationDataUnavailable);
        };
        if guild.owner_id == Some(current_user_id) {
            return None;
        }

        let Some(member) = self.current_member(guild_id, current_user_id) else {
            return (!matches!(level, GuildVerificationLevel::None))
                .then_some(MessageVerificationRestriction::VerificationDataUnavailable);
        };
        if member.pending == Some(true) {
            return Some(MessageVerificationRestriction::MembershipScreening);
        }
        if self.has_full_channel_permissions(channel) {
            return None;
        }
        if member.flags.is_some_and(|flags| {
            flags & MEMBER_FLAG_BYPASSES_VERIFICATION == MEMBER_FLAG_BYPASSES_VERIFICATION
        }) {
            return None;
        }
        if matches!(level, GuildVerificationLevel::None) {
            return None;
        }
        if self.session.current_user_phone_verified == Some(true) {
            return None;
        }
        if matches!(level, GuildVerificationLevel::VeryHigh) {
            return self
                .session
                .current_user_phone_verified
                .map(|_| MessageVerificationRestriction::PhoneVerificationRequired)
                .or(Some(
                    MessageVerificationRestriction::VerificationDataUnavailable,
                ));
        }

        if self.session.current_user_email_verified != Some(true) {
            return self
                .session
                .current_user_email_verified
                .map(|_| MessageVerificationRestriction::EmailVerificationRequired)
                .or(Some(
                    MessageVerificationRestriction::VerificationDataUnavailable,
                ));
        }

        if matches!(
            level,
            GuildVerificationLevel::Medium | GuildVerificationLevel::High
        ) && let Some(remaining_seconds) = minimum_age_remaining_seconds(
            snowflake_created_at(current_user_id),
            ACCOUNT_AGE_SECONDS,
            now,
        ) {
            return Some(MessageVerificationRestriction::AccountTooNew { remaining_seconds });
        }

        if matches!(level, GuildVerificationLevel::High) {
            let Some(joined_at) = member.joined_at else {
                return Some(MessageVerificationRestriction::VerificationDataUnavailable);
            };
            if let Some(remaining_seconds) =
                minimum_age_remaining_seconds(joined_at, MEMBER_AGE_SECONDS, now)
            {
                return Some(MessageVerificationRestriction::MemberTooNew { remaining_seconds });
            }
        }

        None
    }

    fn current_member(
        &self,
        guild_id: Id<GuildMarker>,
        current_user_id: Id<UserMarker>,
    ) -> Option<&GuildMemberState> {
        self.guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&current_user_id))
    }
}

fn snowflake_created_at(user_id: Id<UserMarker>) -> DateTime<Utc> {
    let timestamp_millis = i64::try_from(user_id.get() >> 22)
        .unwrap_or(i64::MAX)
        .saturating_add(DISCORD_EPOCH_MILLIS);
    Utc.timestamp_millis_opt(timestamp_millis)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn minimum_age_remaining_seconds(
    since: DateTime<Utc>,
    minimum_seconds: i64,
    now: DateTime<Utc>,
) -> Option<u64> {
    let remaining_millis =
        (since + chrono::Duration::seconds(minimum_seconds) - now).num_milliseconds();
    (remaining_millis > 0).then(|| {
        u64::try_from(remaining_millis)
            .unwrap_or(u64::MAX)
            .div_ceil(1_000)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{
        AppEvent, ChannelInfo, GuildBoostTier, MemberInfo,
        ids::{Id, marker::ChannelMarker},
    };

    #[test]
    fn verification_levels_apply_their_required_checks() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        let cases = [
            (GuildVerificationLevel::None, 1, 1, false, false, None),
            (
                GuildVerificationLevel::Low,
                60,
                60,
                false,
                false,
                Some(MessageVerificationRestriction::EmailVerificationRequired),
            ),
            (
                GuildVerificationLevel::Medium,
                4,
                60,
                true,
                false,
                Some(MessageVerificationRestriction::AccountTooNew {
                    remaining_seconds: 60,
                }),
            ),
            (
                GuildVerificationLevel::High,
                60,
                9,
                true,
                false,
                Some(MessageVerificationRestriction::MemberTooNew {
                    remaining_seconds: 60,
                }),
            ),
            (
                GuildVerificationLevel::VeryHigh,
                60,
                60,
                true,
                false,
                Some(MessageVerificationRestriction::PhoneVerificationRequired),
            ),
            (GuildVerificationLevel::VeryHigh, 60, 60, true, true, None),
            (GuildVerificationLevel::High, 1, 1, false, true, None),
        ];

        for (level, account_age_minutes, member_age_minutes, email, phone, expected) in cases {
            let (state, channel_id) = verification_state(
                level,
                now,
                account_age_minutes,
                member_age_minutes,
                email,
                phone,
                None,
                Some(false),
            );
            let channel = state.channel(channel_id).expect("channel should exist");
            assert_eq!(
                state.message_verification_restriction_at(channel, now),
                expected,
                "verification level {level:?}"
            );
        }
    }

    #[test]
    fn pending_members_are_blocked_and_bypass_flag_skips_level_checks() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        let (pending, channel_id) = verification_state(
            GuildVerificationLevel::None,
            now,
            60,
            60,
            true,
            true,
            None,
            Some(true),
        );
        assert_eq!(
            pending.message_verification_restriction_at(
                pending.channel(channel_id).expect("channel should exist"),
                now,
            ),
            Some(MessageVerificationRestriction::MembershipScreening)
        );

        let (bypass, channel_id) = verification_state(
            GuildVerificationLevel::VeryHigh,
            now,
            1,
            1,
            false,
            false,
            Some(MEMBER_FLAG_BYPASSES_VERIFICATION),
            Some(false),
        );
        assert_eq!(
            bypass.message_verification_restriction_at(
                bypass.channel(channel_id).expect("channel should exist"),
                now,
            ),
            None
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn verification_state(
        level: GuildVerificationLevel,
        now: DateTime<Utc>,
        account_age_minutes: i64,
        member_age_minutes: i64,
        email_verified: bool,
        phone_verified: bool,
        flags: Option<u64>,
        pending: Option<bool>,
    ) -> (DiscordState, Id<ChannelMarker>) {
        let user_id = snowflake_for_time(now - chrono::Duration::minutes(account_age_minutes));
        let guild_id = Id::new(100);
        let channel_id = Id::new(200);
        let mut channel = ChannelInfo::test(channel_id, "text");
        channel.guild_id = Some(guild_id);
        let mut member = MemberInfo::test(user_id, "neo");
        member.joined_at = Some(now - chrono::Duration::minutes(member_age_minutes));
        member.flags = flags;
        member.pending = pending;

        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(user_id),
        });
        state.apply_event(&AppEvent::CurrentUserVerification {
            email_verified: Some(email_verified),
            phone_verified: Some(phone_verified),
            mfa_enabled: None,
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: None,
            boost_tier: GuildBoostTier::None,
            boost_count: 0,
            verification_level: level,
            mfa_level: 0,
            channels: vec![channel],
            members: vec![member],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        (state, channel_id)
    }

    fn snowflake_for_time(time: DateTime<Utc>) -> Id<UserMarker> {
        let timestamp = u64::try_from(time.timestamp_millis() - DISCORD_EPOCH_MILLIS)
            .expect("test timestamp should follow Discord epoch");
        Id::new((timestamp << 22) | 1)
    }
}
