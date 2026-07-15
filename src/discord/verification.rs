use std::fmt;

use chrono::{DateTime, TimeZone, Utc};

use crate::discord::{
    GuildVerificationLevel, MemberOnboardingStatus,
    guild::GUILD_FEATURE_MEMBER_VERIFICATION_GATE,
    ids::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
    member::MEMBER_FLAG_BYPASSES_VERIFICATION,
    state::{ChannelState, DiscordState, GuildMemberState},
};

const DISCORD_EPOCH_MILLIS: i64 = 1_420_070_400_000;
const ACCOUNT_AGE_SECONDS: i64 = 5 * 60;
const MEMBER_AGE_SECONDS: i64 = 10 * 60;

/// The first local server rule that prevents the current member from
/// participating in a guild channel.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GuildParticipationRestriction {
    MembershipScreening,
    OnboardingIncomplete,
    EmailVerificationRequired,
    AccountTooNew { remaining_seconds: u64 },
    MemberTooNew { remaining_seconds: u64 },
    PhoneVerificationRequired,
    UnsupportedLevel { value: u64 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GuildParticipationDataGap {
    Guild,
    GuildFeatures,
    VerificationLevel,
    CurrentUser,
    CurrentMember,
    MembershipScreeningStatus,
    OnboardingStatus,
    EmailVerificationStatus,
    PhoneVerificationStatus,
    MemberJoinedAt,
}

impl fmt::Display for GuildParticipationDataGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Guild => "the server is not loaded",
            Self::GuildFeatures => "the server features are not loaded",
            Self::VerificationLevel => "the server verification level is not loaded",
            Self::CurrentUser => "the current user is not loaded",
            Self::CurrentMember => "the current server member is not loaded",
            Self::MembershipScreeningStatus => "the membership screening status is not loaded",
            Self::OnboardingStatus => "the onboarding status is not loaded",
            Self::EmailVerificationStatus => "the email verification status is not loaded",
            Self::PhoneVerificationStatus => "the phone verification status is not loaded",
            Self::MemberJoinedAt => "the server join time is not loaded",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum GuildParticipationDecision {
    Allowed,
    Blocked(GuildParticipationRestriction),
    Unavailable(GuildParticipationDataGap),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GuildParticipationBlock {
    Restricted(GuildParticipationRestriction),
    DataUnavailable(GuildParticipationDataGap),
}

impl fmt::Display for GuildParticipationBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restricted(restriction) => restriction.fmt(formatter),
            Self::DataUnavailable(gap) => {
                write!(
                    formatter,
                    "Discord participation status is not available because {gap}"
                )
            }
        }
    }
}

impl fmt::Display for GuildParticipationRestriction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MembershipScreening => write!(
                formatter,
                "complete the server's membership screening in the official Discord app"
            ),
            Self::OnboardingIncomplete => write!(
                formatter,
                "complete the server's onboarding in the official Discord app"
            ),
            Self::EmailVerificationRequired => {
                write!(formatter, "verify the Discord account email")
            }
            Self::AccountTooNew { remaining_seconds } => write!(
                formatter,
                "wait {remaining_seconds} seconds for the Discord account age requirement"
            ),
            Self::MemberTooNew { remaining_seconds } => write!(
                formatter,
                "wait {remaining_seconds} seconds for the server membership age requirement"
            ),
            Self::PhoneVerificationRequired => {
                write!(formatter, "verify the Discord account phone number")
            }
            Self::UnsupportedLevel { value } => {
                write!(
                    formatter,
                    "the server uses unsupported verification level {value}"
                )
            }
        }
    }
}

impl DiscordState {
    pub(crate) fn guild_participation_block(
        &self,
        channel: &ChannelState,
    ) -> Option<GuildParticipationBlock> {
        match self.guild_participation_decision(channel) {
            GuildParticipationDecision::Allowed => None,
            GuildParticipationDecision::Blocked(restriction) => {
                Some(GuildParticipationBlock::Restricted(restriction))
            }
            GuildParticipationDecision::Unavailable(gap) => {
                Some(GuildParticipationBlock::DataUnavailable(gap))
            }
        }
    }

    pub(crate) fn guild_participation_decision(
        &self,
        channel: &ChannelState,
    ) -> GuildParticipationDecision {
        self.guild_participation_decision_at(channel, Utc::now())
    }

    #[cfg(test)]
    pub(crate) fn guild_participation_restriction_at(
        &self,
        channel: &ChannelState,
        now: DateTime<Utc>,
    ) -> Option<GuildParticipationRestriction> {
        match self.guild_participation_decision_at(channel, now) {
            GuildParticipationDecision::Allowed => None,
            GuildParticipationDecision::Blocked(restriction) => Some(restriction),
            GuildParticipationDecision::Unavailable(gap) => {
                panic!("participation data is unavailable in restriction test: {gap:?}")
            }
        }
    }

    pub(crate) fn guild_participation_decision_at(
        &self,
        channel: &ChannelState,
        now: DateTime<Utc>,
    ) -> GuildParticipationDecision {
        let Some(guild_id) = channel.guild_id else {
            return GuildParticipationDecision::Allowed;
        };
        let Some(guild) = self.guild(guild_id) else {
            return GuildParticipationDecision::Unavailable(GuildParticipationDataGap::Guild);
        };
        let Some(level) = guild.verification_level else {
            return GuildParticipationDecision::Unavailable(
                GuildParticipationDataGap::VerificationLevel,
            );
        };
        if let GuildVerificationLevel::Unknown(value) = level {
            return GuildParticipationDecision::Blocked(
                GuildParticipationRestriction::UnsupportedLevel { value },
            );
        }
        let onboarding_may_require_completion = guild.onboarding_may_require_completion();
        let Some(features) = guild.features.as_ref() else {
            return GuildParticipationDecision::Unavailable(
                GuildParticipationDataGap::GuildFeatures,
            );
        };
        let membership_screening_enabled = features
            .iter()
            .any(|feature| feature == GUILD_FEATURE_MEMBER_VERIFICATION_GATE);
        let member_data_required = onboarding_may_require_completion
            || membership_screening_enabled
            || !matches!(level, GuildVerificationLevel::None);
        let Some(current_user_id) = self.current_user_id() else {
            return if member_data_required {
                GuildParticipationDecision::Unavailable(GuildParticipationDataGap::CurrentUser)
            } else {
                GuildParticipationDecision::Allowed
            };
        };
        if guild.owner_id == Some(current_user_id) {
            return GuildParticipationDecision::Allowed;
        }

        let Some(member) = self.current_member(guild_id, current_user_id) else {
            return if member_data_required {
                GuildParticipationDecision::Unavailable(GuildParticipationDataGap::CurrentMember)
            } else {
                GuildParticipationDecision::Allowed
            };
        };
        if member.pending == Some(true) {
            return GuildParticipationDecision::Blocked(
                GuildParticipationRestriction::MembershipScreening,
            );
        }
        if self.has_full_channel_permissions(channel) {
            return GuildParticipationDecision::Allowed;
        }
        if member.flags.is_some_and(|flags| {
            flags & MEMBER_FLAG_BYPASSES_VERIFICATION == MEMBER_FLAG_BYPASSES_VERIFICATION
        }) {
            return GuildParticipationDecision::Allowed;
        }
        if member.pending.is_none() && membership_screening_enabled {
            return GuildParticipationDecision::Unavailable(
                GuildParticipationDataGap::MembershipScreeningStatus,
            );
        }
        if onboarding_may_require_completion {
            match member.onboarding_status() {
                Some(MemberOnboardingStatus::Completed) => {}
                Some(MemberOnboardingStatus::NotStarted | MemberOnboardingStatus::InProgress) => {
                    return GuildParticipationDecision::Blocked(
                        GuildParticipationRestriction::OnboardingIncomplete,
                    );
                }
                None => {
                    return GuildParticipationDecision::Unavailable(
                        GuildParticipationDataGap::OnboardingStatus,
                    );
                }
            }
        }
        // Assigned roles exempt members from the classic guild verification
        // level. Membership screening and enabled onboarding remain separate
        // restrictions and are checked before this exemption.
        if !member.role_ids.is_empty() {
            return GuildParticipationDecision::Allowed;
        }
        if matches!(level, GuildVerificationLevel::None) {
            return GuildParticipationDecision::Allowed;
        }
        if self.session.current_user_phone_verified == Some(true) {
            return GuildParticipationDecision::Allowed;
        }
        if matches!(level, GuildVerificationLevel::VeryHigh) {
            return match self.session.current_user_phone_verified {
                Some(_) => GuildParticipationDecision::Blocked(
                    GuildParticipationRestriction::PhoneVerificationRequired,
                ),
                None => GuildParticipationDecision::Unavailable(
                    GuildParticipationDataGap::PhoneVerificationStatus,
                ),
            };
        }

        if self.session.current_user_email_verified != Some(true) {
            return match self.session.current_user_email_verified {
                Some(_) => GuildParticipationDecision::Blocked(
                    GuildParticipationRestriction::EmailVerificationRequired,
                ),
                None => GuildParticipationDecision::Unavailable(
                    GuildParticipationDataGap::EmailVerificationStatus,
                ),
            };
        }

        if matches!(
            level,
            GuildVerificationLevel::Medium | GuildVerificationLevel::High
        ) && let Some(remaining_seconds) = minimum_age_remaining_seconds(
            snowflake_created_at(current_user_id),
            ACCOUNT_AGE_SECONDS,
            now,
        ) {
            return GuildParticipationDecision::Blocked(
                GuildParticipationRestriction::AccountTooNew { remaining_seconds },
            );
        }

        if matches!(level, GuildVerificationLevel::High) {
            let Some(joined_at) = member.joined_at else {
                return GuildParticipationDecision::Unavailable(
                    GuildParticipationDataGap::MemberJoinedAt,
                );
            };
            if let Some(remaining_seconds) =
                minimum_age_remaining_seconds(joined_at, MEMBER_AGE_SECONDS, now)
            {
                return GuildParticipationDecision::Blocked(
                    GuildParticipationRestriction::MemberTooNew { remaining_seconds },
                );
            }
        }

        GuildParticipationDecision::Allowed
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
        AppEvent, ChannelInfo, GuildBoostTier, GuildOnboardingInfo, GuildOnboardingMode,
        MemberInfo, MemberOnboardingStatus,
        ids::{Id, marker::ChannelMarker},
        member::{MEMBER_FLAG_COMPLETED_ONBOARDING, MEMBER_FLAG_STARTED_ONBOARDING},
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn verification_and_membership_rules_apply_in_order() {
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
                Some(GuildParticipationRestriction::EmailVerificationRequired),
            ),
            (
                GuildVerificationLevel::Medium,
                4,
                60,
                true,
                false,
                Some(GuildParticipationRestriction::AccountTooNew {
                    remaining_seconds: 60,
                }),
            ),
            (
                GuildVerificationLevel::High,
                60,
                9,
                true,
                false,
                Some(GuildParticipationRestriction::MemberTooNew {
                    remaining_seconds: 60,
                }),
            ),
            (
                GuildVerificationLevel::VeryHigh,
                60,
                60,
                true,
                false,
                Some(GuildParticipationRestriction::PhoneVerificationRequired),
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
                state.guild_participation_restriction_at(channel, now),
                expected,
                "verification level {level:?}"
            );
        }

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
            pending.guild_participation_restriction_at(
                pending.channel(channel_id).expect("channel should exist"),
                now,
            ),
            Some(GuildParticipationRestriction::MembershipScreening)
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
            bypass.guild_participation_restriction_at(
                bypass.channel(channel_id).expect("channel should exist"),
                now,
            ),
            None
        );
    }

    #[test]
    fn participation_data_gaps_fail_closed() {
        let guild_id = Id::new(100);
        let channel_id = Id::new(200);
        let mut channel = ChannelInfo::test(channel_id, "GuildText");
        channel.guild_id = Some(guild_id);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel));

        let decision = state
            .guild_participation_decision(state.channel(channel_id).expect("channel should exist"));

        assert_eq!(
            decision,
            GuildParticipationDecision::Unavailable(GuildParticipationDataGap::Guild)
        );

        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        let (mut state, channel_id) = verification_state(
            GuildVerificationLevel::None,
            now,
            60,
            60,
            true,
            true,
            Some(MEMBER_FLAG_COMPLETED_ONBOARDING),
            Some(false),
        );
        state.apply_event(&AppEvent::GuildOnboardingUpdate {
            guild_id: Id::new(100),
            onboarding: onboarding(Id::new(100), true),
        });
        let user_id = state.current_user_id().expect("current user should exist");
        state.apply_event(&AppEvent::GuildMemberRemove {
            guild_id: Id::new(100),
            user_id,
        });

        assert_eq!(
            state.guild_participation_decision_at(
                state.channel(channel_id).expect("channel should exist"),
                now,
            ),
            GuildParticipationDecision::Unavailable(GuildParticipationDataGap::CurrentMember)
        );

        for (pending, expected) in [
            (
                None,
                GuildParticipationDecision::Unavailable(
                    GuildParticipationDataGap::MembershipScreeningStatus,
                ),
            ),
            (Some(false), GuildParticipationDecision::Allowed),
        ] {
            let (mut state, channel_id) = verification_state(
                GuildVerificationLevel::None,
                now,
                60,
                60,
                true,
                true,
                Some(MEMBER_FLAG_COMPLETED_ONBOARDING),
                pending,
            );
            update_guild_features(&mut state, &["MEMBER_VERIFICATION_GATE_ENABLED"]);

            assert_eq!(
                state.guild_participation_decision_at(
                    state.channel(channel_id).expect("channel should exist"),
                    now,
                ),
                expected,
                "pending state {pending:?}"
            );
        }
    }

    #[test]
    fn assigned_roles_bypass_classic_verification_after_onboarding_checks() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        let cases = [None, Some(true), Some(false)];

        for onboarding_enabled in cases {
            let (mut state, channel_id) = verification_state(
                GuildVerificationLevel::Low,
                now,
                60,
                60,
                false,
                false,
                Some(0),
                Some(false),
            );
            let user_id = state.current_user_id().expect("current user should exist");
            let mut member = MemberInfo::test(user_id, "neo");
            member.role_ids = vec![Id::new(300)];
            member.flags = Some(MEMBER_FLAG_COMPLETED_ONBOARDING);
            state.apply_event(&AppEvent::GuildMemberUpsert {
                guild_id: Id::new(100),
                member,
            });
            if let Some(enabled) = onboarding_enabled {
                state.apply_event(&AppEvent::GuildOnboardingUpdate {
                    guild_id: Id::new(100),
                    onboarding: onboarding(Id::new(100), enabled),
                });
            }

            assert_eq!(
                state.guild_participation_restriction_at(
                    state.channel(channel_id).expect("channel should exist"),
                    now,
                ),
                None,
                "onboarding enabled state {onboarding_enabled:?}"
            );
        }
    }

    #[test]
    fn member_flags_drive_onboarding_status_and_participation() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        let cases = [
            (
                Some(0),
                Some(MemberOnboardingStatus::NotStarted),
                Some(false),
                GuildParticipationDecision::Blocked(
                    GuildParticipationRestriction::OnboardingIncomplete,
                ),
            ),
            (
                Some(MEMBER_FLAG_STARTED_ONBOARDING),
                Some(MemberOnboardingStatus::InProgress),
                Some(true),
                GuildParticipationDecision::Blocked(
                    GuildParticipationRestriction::OnboardingIncomplete,
                ),
            ),
            (
                Some(MEMBER_FLAG_STARTED_ONBOARDING | MEMBER_FLAG_COMPLETED_ONBOARDING),
                Some(MemberOnboardingStatus::Completed),
                Some(false),
                GuildParticipationDecision::Allowed,
            ),
            (
                None,
                None,
                None,
                GuildParticipationDecision::Unavailable(
                    GuildParticipationDataGap::OnboardingStatus,
                ),
            ),
        ];

        for (flags, expected_status, expected_active, expected_decision) in cases {
            let (mut state, channel_id) = verification_state(
                GuildVerificationLevel::None,
                now,
                60,
                60,
                true,
                true,
                flags,
                Some(false),
            );
            state.apply_event(&AppEvent::GuildOnboardingUpdate {
                guild_id: Id::new(100),
                onboarding: onboarding(Id::new(100), true),
            });

            assert_eq!(
                state.current_user_onboarding_status(Id::new(100)),
                expected_status
            );
            assert_eq!(
                state.current_user_is_onboarding(Id::new(100)),
                expected_active
            );
            assert_eq!(
                state.guild_participation_decision_at(
                    state.channel(channel_id).expect("channel should exist"),
                    now,
                ),
                expected_decision,
                "member flags {flags:?}"
            );
        }
    }

    #[test]
    fn community_capability_does_not_imply_enabled_onboarding() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("test time should be valid");
        for flags in [Some(0), Some(MEMBER_FLAG_STARTED_ONBOARDING), None] {
            let (mut state, channel_id) = verification_state(
                GuildVerificationLevel::None,
                now,
                60,
                60,
                true,
                true,
                flags,
                Some(false),
            );
            update_guild_features(&mut state, &["COMMUNITY"]);

            assert_eq!(
                state.guild_participation_decision_at(
                    state.channel(channel_id).expect("channel should exist"),
                    now,
                ),
                GuildParticipationDecision::Allowed,
                "member flags {flags:?}"
            );
            assert_eq!(
                state.current_user_is_onboarding(Id::new(100)),
                Some(false),
                "member flags {flags:?}"
            );
        }
    }

    fn update_guild_features(state: &mut DiscordState, features: &[&str]) {
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id: Id::new(100),
            name: "guild".to_owned(),
            owner_id: None,
            boost_tier: None,
            boost_count: None,
            verification_level: None,
            mfa_level: None,
            features: Some(
                features
                    .iter()
                    .map(|feature| (*feature).to_owned())
                    .collect(),
            ),
            onboarding: None,
            roles: None,
            emojis: None,
        });
    }

    fn onboarding(guild_id: Id<GuildMarker>, enabled: bool) -> GuildOnboardingInfo {
        GuildOnboardingInfo {
            guild_id,
            enabled: Some(enabled),
            mode: Some(GuildOnboardingMode::Default),
            default_channel_ids: Vec::new(),
            raw: Arc::new(json!({
                "guild_id": guild_id.to_string(),
                "enabled": enabled,
                "mode": 0,
                "default_channel_ids": [],
                "prompts": []
            })),
        }
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
            verification_level: Some(level),
            mfa_level: Some(0),
            features: Some(Vec::new()),
            onboarding: None,
            channels: vec![channel],
            members: vec![member],
            presences: Vec::new(),
            roles: Some(Vec::new()),
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
