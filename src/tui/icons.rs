//! Application-owned glyphs. Content emoji, input syntax, and widget geometry
//! stay outside this palette so changing fonts never changes a user's message.

use crate::config::IconSet;

macro_rules! define_icons {
    ($($name:ident => ($unicode:literal, $nerd:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(super) enum Icon {
            $($name),+
        }

        impl Icon {
            #[cfg(test)]
            const ALL: &[Self] = &[$(Self::$name),+];

            pub(super) const fn for_set(self, set: IconSet) -> &'static str {
                let (unicode, nerd) = match self {
                    $(Self::$name => ($unicode, $nerd)),+
                };
                match set {
                    IconSet::Unicode => unicode,
                    IconSet::Nerd => nerd,
                }
            }

            pub(super) fn glyph(self) -> &'static str {
                self.for_set(super::theme::current().icon_set())
            }
        }
    };
}

// Code points are from Nerd Fonts 3.x glyphnames.json. Use a Nerd Font Mono
// terminal font for single-cell icons. The Unicode column preserves the old UI.
// https://github.com/ryanoasis/nerd-fonts/blob/v3.0.0/glyphnames.json
define_icons! {
    Selection => ("▸", "\u{f0142}"), // md-chevron_right
    Expanded => ("▼", "\u{f0140}"), // md-chevron_down
    Collapsed => ("▶", "\u{f0142}"), // md-chevron_right
    FolderClosed => ("📁", "\u{f024b}"), // md-folder
    FolderOpen => ("📂", "\u{f0770}"), // md-folder_open
    DirectMessage => ("@", "\u{f0004}"), // md-account
    GroupMessage => ("👥", "\u{f0849}"), // md-account_group
    TextChannel => ("#", "\u{f0423}"), // md-pound
    VoiceChannel => ("🔈", "\u{f057e}"), // md-volume_high
    VoiceActive => ("🔊", "\u{f05cb}"), // md-account_voice
    Category => ("▾", "\u{f0140}"), // md-chevron_down
    Forum => ("📝", "\u{f028c}"), // md-forum
    Tag => ("#", "\u{f04f9}"), // md-tag
    ThreadChannel => ("»", "\u{f0369}"), // md-message_text
    ThreadPost => ("💬", "\u{f0369}"), // md-message_text
    ThreadList => ("🧵", "\u{f0369}"), // md-message_text
    PresenceMobile => ("▮", "\u{f10b}"), // fa-mobile_phone
    PresenceOnline => ("●", "\u{f111}"), // fa-circle
    PresenceIdle => ("●", "\u{f0594}"), // md-weather_night
    PresenceDnd => ("●", "\u{f0376}"), // md-minus_circle
    PresenceOffline => ("○", "\u{f10c}"), // fa-circle_o
    Boost => ("⚡", "\u{f140b}"), // md-lightning_bolt
    StreamLive => ("🔴", "\u{f0567}"), // md-video
    StreamPaused => ("⏸", "\u{f03e4}"), // md-pause
    VoiceMuted => ("🔇", "\u{f036d}"), // md-microphone_off
    VoiceDeafened => ("🎧", "\u{f07ce}"), // md-headphones_off
    ActivityPlaying => ("▶", "\u{f0297}"), // md-gamepad_variant
    ActivityStreaming => ("◉", "\u{f0567}"), // md-video
    ActivityListening => ("♪", "\u{f075a}"), // md-music
    ActivityWatching => ("▷", "\u{f0567}"), // md-video
    ChevronRight => ("›", "\u{f0142}"), // md-chevron_right
    Search => ("🔎", "\u{f0349}"), // md-magnify
    Filter => ("/", "\u{f0232}"), // md-filter
    Prompt => (">", "\u{f0142}"), // md-chevron_right
    MentionRole => ("@", "\u{f0065}"), // md-at
    Pinned => ("★", "\u{f04ce}"), // md-star
    Loading => ("⠋", "\u{f0772}"), // md-loading
    CustomEmoji => ("◆", "\u{f01f2}"), // md-emoticon_outline
    AnimatedEmoji => ("◇", "\u{f093a}"), // md-animation_play
    Checked => ("[x]", "\u{f0132}"), // md-checkbox_marked
    Unchecked => ("[ ]", "\u{f0131}"), // md-checkbox_blank_outline
    Relationship => ("●", "\u{f0004}"), // md-account
    ListItem => ("•", "\u{f09df}"), // md-circle_small
    NewMessages => ("↓", "\u{f0045}"), // md-arrow_down
    PollSelected => ("◉", "\u{f043e}"), // md-radiobox_marked
    PollUnselected => ("◯", "\u{f10c}"), // fa-circle_o
    Reply => ("↳", "\u{f045a}"), // md-reply
    Forward => ("↱", "\u{f0496}"), // md-share
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn nerd_palette_uses_single_cell_icons_for_every_application_symbol() {
        for &icon in Icon::ALL {
            let unicode = icon.for_set(IconSet::Unicode);
            let nerd = icon.for_set(IconSet::Nerd);
            assert!(!unicode.is_empty(), "{icon:?}");
            assert_ne!(unicode, nerd, "{icon:?}");
            assert_eq!(nerd.chars().count(), 1, "{icon:?}");
            assert_eq!(nerd.width(), 1, "{icon:?}");
        }
    }
}
