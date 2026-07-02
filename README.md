# Concord

<img width="1613" height="848" alt="concord - a feature-rich TUI client for
  Discord" src="./docs/example.png" />

Concord is a feature-rich TUI (terminal user interface) client for Discord, written in Rust with ratatui. Full Discord experience, right in your terminal.

## Table of contents

- [Installation](#installation)
- [Features](#features)
- [Configuration](#configuration)
- [Performance](#performance)
- [FAQ](#faq)
- [Security](#security)
- [Contributing](#contributing)
- [License](#license)

## Installation

### Homebrew

```sh
brew install concord
```

Or with the tap for the latest version:

```sh
brew install chojs23/tap/concord
```

### npm

```sh
npm install -g @chojs23/concord
```

You can also run Concord without a global install:

```sh
npx @chojs23/concord
```

The npm package installs a prebuilt binary from the GitHub Release artifacts.

### Cargo

Install native dependencies first.

On macOS with Homebrew:

```sh
brew install opus pkg-config
```

On Fedora:

```sh
sudo dnf install opus-devel alsa-lib-devel pkgconf-pkg-config
```

On Debian or Ubuntu:

```sh
sudo apt install libopus-dev libasound2-dev pkg-config
```

```sh
cargo install concord --locked
```

To install without local voice playback and microphone support:

```sh
cargo install concord --locked --no-default-features
```

To install the latest unreleased version directly from the Git repository:

```sh
cargo install --locked --git https://github.com/chojs23/concord
```

### Nix

Install the packaged release from nixpkgs:

```sh
nix profile install nixpkgs#concord-tui
```

Run the packaged release without installing:

```sh
nix run nixpkgs#concord-tui
```

Run the latest release without installing (requires flakes enabled):

```sh
nix run github:chojs23/concord
```

Install into your profile:

```sh
nix profile install github:chojs23/concord
```

Or add the flake as an input in your own `flake.nix`:

```nix
{
  inputs.concord.url = "github:chojs23/concord";
}
```

### GitHub Release installer

Install the latest release with the cargo-dist shell installer:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/chojs23/concord/releases/latest/download/concord-installer.sh | sh
```

On Windows, use the PowerShell installer instead:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/chojs23/concord/releases/latest/download/concord-installer.ps1 | iex"
```

The installer places `concord` under `$CARGO_HOME/bin`, which is usually
`~/.cargo/bin` on Unix and `%USERPROFILE%\.cargo\bin` on Windows.

### Build from source

You need the Rust stable toolchain, Cargo, and the native dependencies listed in
the Cargo install section.

```sh
git clone https://github.com/chojs23/concord.git
cd concord
cargo build --release
```

The release binary is produced at:

```sh
target/release/concord
```

To build without local voice playback and microphone support, disable default features:

```sh
cargo build --release --no-default-features
```

On WSLg, audio is usually exposed through PulseAudio instead of a real ALSA
sound card. If playback does not start, check that PulseAudio and ALSA routing
work before debugging Discord voice itself:

```sh
pactl info
paplay /usr/share/sounds/alsa/Front_Center.wav
aplay -D pulse /usr/share/sounds/alsa/Front_Center.wav
```

## Features

### Authentication

- **Token** : paste an existing Discord token.
- **Email / Password** : login with credentials. MFA (TOTP, SMS) is fully supported.
- **QR Code** : scan the code from the Discord mobile app.
- **CONCORD_TOKEN env var** : set `CONCORD_TOKEN=your-token` before launching Concord. Takes precedence over the plaintext credential file when `credentials.store = "auto"`, but the keychain is checked first.

Email and QR code logins may trigger a CAPTCHA challenge on Discord's side. We cannot solve that, so I strongly recommend using token authentication.

By default, tokens are saved in the system keychain when available. In the
default `auto` mode, Concord falls back to its state directory when keychain
storage is unavailable. See the Security section below for details.

To sign out with the default keys, open your profile with `Space`, `p`, then
choose `[o] sign out`.

### Guilds & Channels

- Browse servers with guild folder grouping
- Navigate text channels, threads, and forum channels
- View, filter, and create forum/media posts (active / archived)
- Create, edit, follow, close, lock, pin, and delete threads and forum posts
- Switch channels, threads, and posts with the fuzzy channel switcher (`Space`, `Space`)
- Load pinned messages per channel
- Open channel actions for pinned messages, thread lists, and mark-as-read
- Join and leave voice channels/DM calls
- Highlight active voice speakers in voice channel/DM calls
- Track unread messages and mention counts per channel
- Mute and unmute channels and servers
- Leave the selected server after confirmation

### Messaging

- Send, edit, and delete messages
- Upload / Download attachments
- Search messages with filters with `/`
- Use @mention autocomplete
- Use custom emoji from other servers when your account supports it
- Send custom emoji your account cannot use directly as image links when enabled
- View full message history
- Rich content display (embeds, attachments, stickers, and mentions)
- Detect URLs in message bodies and markdown links, then open them in your default browser
- Direct message shortcuts for copy, reply, edit, delete, reactions, URL opening,
  and image viewing. More message actions are available from the action menu.

#### Markdown Rendering & Code syntax highlighting

![Markdown rendering example](./docs/markdown-example.png)

Concord renders a practical subset of Discord-style Markdown in message bodies:

- Headings: `# H1`, `## H2`, `### H3`
- Quotes: `> quoted text`
- Bullets: `- item` and `* item`
- Inline styles: `**bold**`, `*italic*`, `__underline__`, `~~strikethrough~~`, and `` `inline code` ``
- Fenced code blocks with optional language labels, rendered as compact boxes with syntax highlighting
- Raw URLs and markdown link destinations are underlined and can be opened from message actions

### Reactions & Polls

- View, add, and remove emoji reactions (Unicode and custom server emoji)
- Use custom emoji from other servers as reactions when your account supports it
- Browse who reacted with a specific emoji
- View and vote on polls

### Media & Images

Image rendering is powered by [ratatui-image](https://github.com/benjajaja/ratatui-image). On startup, Concord queries the terminal to detect the best available graphics protocol. Supported protocols:

- **Kitty Graphics Protocol** - Kitty, WezTerm, Ghostty, etc.
- **iTerm2 Inline Images** - iTerm2, WezTerm, mintty, etc.
- **Sixel** - foot, mlterm, xterm (if compiled with Sixel support), etc.
- **Halfblocks** (fallback) - works on any terminal, but uses block characters instead of true pixels.

If your terminal does not support any graphics protocol, images will be rendered as halfblock approximations. For the best experience, use a terminal that supports the Kitty or iTerm2 protocol.

You can toggle image viewing on or off in the configuration file. When image viewing is off, attachments and emojis will be shown as text placeholders.

Video and audio playback uses [mpv](https://mpv.io/). Make sure `mpv` is installed and in your PATH.
YouTube playback depends on your local `mpv` setup, such as `yt-dlp` support.
External media playback is off by default. You can enable it with
`media_playback = true` under `[display]`, or toggle it from the in-app Display
options menu.

### Members & Profiles

- Member list with grouping
- Search members in server
- Presence indicators (Online, Idle, DND, Offline)
- Configure user profile and custom activity status

### Rich Presence

- Concord serves the local `discord-ipc` socket, detects connected apps, and
  lets you pick which one to share from your profile's activity picker
- Only apps that speak Discord's Rich Presence (RPC/IPC) protocol are detected.
- Detection needs the official Discord client closed, since only one program can
  own the socket and the official client claims it first
- Toggle with `share_rich_presence` under `[presence]` in `config.toml`

### Typing Indicators & Read State

- Live "user is typing..." indicators
- Unread message tracking with mention counts
- Mark server, channel as read

### Notifications

- Notification inbox (`<leader>n`) with Unreads and Mentions tabs, per-channel
  message previews, and mark-as-read / mark-all-read
- You can configure notification sounds with custom WAV files
- Desktop notifications for messages that pass your Discord
  notification settings
- Voice join and leave notification sounds while you are connected to voice

### Navigation & Keyboard shortcuts

> ⚠️ Keymap action names and default bindings may have breaking changes between releases.

All default key settings in this section can be customized. See
[Keymap options](./docs/keymap-options.md) for the config format and supported
actions.

Concord has a four-pane.
**Guilds (1)**, **Channels (2)**, **Messages (3)**, **Members (4)**

With default vim-style navigation:

| Key                                       | Action                                          |
| ----------------------------------------- | ----------------------------------------------- |
| `1` `2` `3` `4`                           | Focus pane                                      |
| `Tab` / `Shift+Tab`                       | Cycle focus forward / backward                  |
| `h` / `l`, `←` / `→`                      | Move focus left / right                         |
| `j` / `k`, `↑` / `↓`, `Ctrl+n` / `Ctrl+p` | Move down / up                                  |
| `J`, `K` / `H`, `L`                       | Scroll viewport                                 |
| `Ctrl+d` / `Ctrl+u`                       | Half-page scroll                                |
| `Alt+h/l/←/→`                             | Resize focused pane width                       |
| `gg` / `G`                                | Jump or scroll to top / bottom                  |
| `Enter`                                   | Open or activate the selected item              |
| `/`                                       | Filter Guilds/Channels, search Messages/Members |
| `Space`                                   | Open leader shortcut window                     |
| `i`                                       | Text insert mode, or forum post composer        |
| `Esc` / `q`                               | Close popup, cancel mode, or go back            |
| `q`                                       | Quit Concord                                    |

`Ctrl+n` and `Ctrl+p` are fixed row movement keys. The default `j` and `k`
row movement keys are `SelectNext` and `SelectPrevious` and can be changed in
`keymap.toml`.

#### Leader key

Press `Space` to open the leader shortcut window.

| Key sequence     | Action                            |
| ---------------- | --------------------------------- |
| `Space`, `1`     | Toggle the Servers pane           |
| `Space`, `2`     | Toggle the Channels pane          |
| `Space`, `4`     | Toggle the Members pane           |
| `Space`, `a`     | Open actions for the focused pane |
| `Space`, `p`     | Open my profile settings          |
| `Space`, `o`     | Choose concord option category    |
| `Space`, `n`     | Open the notification inbox       |
| `Space`, `v`     | Voice command prefix              |
| `Space`, `Space` | Open the fuzzy channel switcher   |

#### Action menus

Focus a pane, then press `Space`, `a` to open actions for that pane. Action
shortcuts are shown inside the leader popup and only run when the action is
enabled. In the Messages pane, the selected message also supports these direct
shortcuts:

Message shortcuts:

| Shortcut | Action              | Description                                                 |
| -------- | ------------------- | ----------------------------------------------------------- |
| `y`      | Copy                | Copy the selected message text and show a short toast       |
| `r`      | Add/remove reaction | Open the reaction picker for the selected message           |
| `R`      | Reply               | Start a reply to the selected message                       |
| `d`      | Delete              | Open a delete confirmation before deleting the message      |
| `e`      | Edit                | Start editing the selected message when editing is allowed  |
| `o`      | Open URL            | Open the selected message URL, or choose from multiple URLs |
| `x`      | Play media          | Play selected video or audio media in an external player    |
| `v`      | View attachment     | Open the selected message's attachment viewer               |

Message action menu shortcuts:

| Shortcut | Action                      | Description                                                 |
| -------- | --------------------------- | ----------------------------------------------------------- |
| `y`      | Copy                        | Copy the selected message text and show a short toast       |
| `r`      | Add/remove reaction         | Open the reaction picker for the selected message           |
| `R`      | Reply                       | Start a reply to the selected message                       |
| `d`      | Delete                      | Open a delete confirmation before deleting the message      |
| `e`      | Edit                        | Start editing the selected message when editing is allowed  |
| `o`      | Open URL                    | Open the selected message URL, or choose from multiple URLs |
| `D`      | Remove embeds               | Remove embeds from the selected message                     |
| `x`      | Play media                  | Play selected video or audio media in an external player    |
| `v`      | View attachment             | Open the selected message's attachment viewer               |
| `g`      | Go to referenced message    | Go to the replied or forwarded message                      |
| `p`      | show message sender profile | Open the selected message author's profile                  |
| `P`      | Pin / unpin                 | Open a pin or unpin confirmation for the selected message   |
| `t`      | Open thread                 | Open the selected message's thread                          |
| `u`      | Show reacted users          | Show users who reacted to the selected message              |
| `c`      | Choose poll votes           | Choose poll votes for the selected message                  |

When the attachment viewer is open, press `x` to play the current video or audio attachment
in an external player, or `d` to download the current attachment directly.

Server actions:

| Shortcut | Action              | Description                                           |
| -------- | ------------------- | ----------------------------------------------------- |
| `m`      | Mark server as read | Mark all unread viewable channels in this server read |
| `u`      | Mute / unmute       | Toggle server notification mute                       |
| `l`      | Leave server        | Open a confirmation before leaving this server        |
| `r`      | Folder settings     | Edit the selected server folder name and color        |

Channel actions:

| Shortcut | Action               | Description                                  |
| -------- | -------------------- | -------------------------------------------- |
| `j`      | Join voice           | Join the selected voice channel or DM call   |
| `l`      | Leave voice          | Leave the current voice channel or DM call   |
| `p`      | Show pinned messages | Open the selected channel's pinned messages  |
| `t`      | Show threads         | List threads for the selected channel        |
| `m`      | Mark as read         | Mark the selected channel read               |
| `u`      | Mute / unmute        | Toggle channel or category notification mute |

Thread / post actions (a focused thread in the Channels pane or a forum post in the post list):

| Shortcut | Action                | Description                                                 |
| -------- | --------------------- | ----------------------------------------------------------- |
| `m`      | Mark as read          | Mark the thread or post read                                |
| `f`      | Follow / unfollow     | Follow or unfollow the thread or post                       |
| `c`      | Close / reopen        | Close or reopen the thread or post (author or moderator)    |
| `l`      | Lock / unlock         | Lock or unlock the thread or post (moderator)               |
| `e`      | Edit                  | Edit title, tags, slow mode, and auto-archive               |
| `y`      | Copy link             | Copy a link to the thread or post                           |
| `u`      | Mute / unmute         | Toggle thread or post notification mute (must follow first) |
| `n`      | Notification settings | Choose the notification level for the thread or post        |
| `P`      | Pin / unpin           | Pin or unpin the post (forum posts only, moderator)         |
| `d`      | Delete                | Delete the whole thread or post (moderator)                 |
| `i`      | Copy ID               | Copy the thread or post ID                                  |

Voice commands:

| Sequence          | Action       | Description                               |
| ----------------- | ------------ | ----------------------------------------- |
| `Space`, `v`, `d` | Deafen voice | Toggle Concord's Discord voice deaf state |
| `Space`, `v`, `m` | Mute voice   | Toggle Concord's Discord voice mute state |
| `Space`, `v`, `l` | Leave voice  | Leave the current Concord voice channel   |

#### Composer

You can paste copied files into the composer to attach them. Pending uploads
are shown above the input before sending.

In a forum or media parent channel, the same composer key opens a post composer
overlay.

| Shortcut                                     | Action            | Description                                                      |
| -------------------------------------------- | ----------------- | ---------------------------------------------------------------- |
| `Ctrl+v`                                     | paste clipboard   | Attach copied files or images when present, otherwise paste text |
| `Ctrl+e`                                     | open $EDITOR      | Open $EDITOR on the current draft for long editing               |
| `Ctrl+c`                                     | clear             | Clear current draft                                              |
| `Ctrl+Left`/ `Ctrl+Right`                    | Jump word         | Jump the cursor by word                                          |
| `Alt+Backspace`, `Ctrl+Backspace` / `Ctrl+w` | Delete word       | Delete the word before the cursor                                |
| `Delete`                                     | Detach attachment | Removes the last pending attachment                              |
| `Alt+p`                                      | Toggle reply ping | Toggle whether replying pings the original author                |

#### Mention picker

When the @mention picker is open, use `Up` / `Down`,
`Ctrl+p` / `Ctrl+n`, `Tab`, or `Enter` to choose a mention.

#### Emoji picker

Type `:` plus at least two letters, such as `:he`, to pick Unicode, server, or
cross-server custom emoji while writing a message.

When `emojis_as_links` is enabled, custom emoji your account cannot send
directly are inserted as Discord image links instead.

To react from the composer, select a message, enter insert mode, then type
`+:`. The reaction picker opens for that message. Press `/` to search, Enter to
lock the filter, then Enter again or use a shown shortcut to react.

#### Bot commands

When the composer input starts with a slash `/`, the command suggestion popup

## Configuration

Concord options are stored under Concord's config directory. If
`XDG_CONFIG_HOME` is set, Concord uses `config.toml` for app options and `keymap.toml` for key settings.
Otherwise it uses the platform config directory.

Local UI state and plaintext fallback credentials are stored under Concord's
state directory instead. If `XDG_STATE_HOME` is set, Concord uses
`$XDG_STATE_HOME/concord/state.toml` and
`$XDG_STATE_HOME/concord/credentials.toml`. Otherwise it uses
`~/.local/state/concord/state.toml` and
`~/.local/state/concord/credentials.toml`.

You can change some configuration from the in-app Options menu, and Concord saves them back
to `config.toml`. Key settings are read from `keymap.toml`.

<details>
<summary>Default config</summary>

```toml
[display]
# Image protocol: auto, iterm2, kitty, sixel, or halfblocks.
image_protocol = "auto"

# Master switch that hides all image previews when true.
disable_image_preview = false

# Show user avatars next to messages and in profile views.
show_avatars = true

# Render inline image previews for attachments and embeds.
show_images = true

# Allow video and audio media to open in an external player.
media_playback = false

# Preview quality: efficient, balanced, high, or original.
image_preview_quality = "balanced"

# Attachment viewer quality: efficient, balanced, high, or original.
attachment_viewer_quality = "original"

# Render custom Discord emoji as images when possible.
show_custom_emoji = true

# Crop avatars into circles instead of showing square images.
circular_avatars = false

[composer]
# Send custom emoji your account cannot use directly as image links.
emojis_as_links = false

[presence]
# Relay Rich Presence from local apps as your activity.
share_rich_presence = true

[credentials]
# Credential storage: auto, keychain, or plain.
# auto tries the system keychain first and falls back to the state file.
store = "auto"

[notifications]
# Show desktop notifications for Discord messages that pass notification rules.
desktop_notifications = true

# Optional notification icon to include in notifications. May not work on all platforms.
# When unset, no icon is used. It must either be a name of an icon (typically in /usr/share/icons)
# or a path to an icon.
notification_icon = "/path/to/icon.svg"

# Optional WAV files for message, voice join/leave notification sounds.
# When unset, Concord uses built-in generated tones.
notification_sound = "/path/to/message.wav"
voice_join_sound = "/path/to/join.wav"
voice_leave_sound = "/path/to/leave.wav"

[voice]
# Join or update Discord voice with Concord self-muted.
self_mute = false

# Join or update Discord voice with Concord self-deafened.
self_deaf = false

# Allow microphone transmit while this session is joined and not self-muted.
allow_microphone_transmit = false

# Voice activity threshold in dB. Lower values transmit quieter input.
microphone_sensitivity = -30

# Microphone input volume percentage, from 0 to 100.
microphone_volume = 100

# Received voice playback volume percentage, from 0 to 100.
voice_output_volume = 100
```

</details><br>

`image_protocol`:

- `auto`: use terminal detection. In iTerm, Concord uses the iTerm2 protocol
  because terminal capability detection can incorrectly select Kitty there.
- `iterm2`: force the iTerm2 inline image protocol.
- `kitty`: force the Kitty graphics protocol.
- `sixel`: force Sixel rendering.
- `halfblocks`: force Unicode half-block fallback rendering.

`image_preview_quality` and `attachment_viewer_quality`:

- `efficient`: smaller preview requests to reduce bandwidth and memory use.
- `balanced`: default quality with bounded resource use.
- `high`: sharper resized previews using lossless quality.
- `original`: request the original source image for previews when possible.

This setting only applies to attachment, embed, and attachment viewer previews.
Avatars and custom emoji keep their separate small-image behavior.

`credentials.store`:

- `auto`: try the system keychain first, then fall back to the state-file credential store.
- `keychain`: use only the system keychain. If keychain storage is unavailable, the token is not saved.
- `plain`: use only the plain-text state-file credential store.

<details>
<summary>Default keymap config</summary>

```toml
[keymap]
leader = "space"
StartComposer = "i"
OpenPaneFilter = "/"
ClosePopup = "q"
FocusGuildPane = "1"
FocusChannelPane = "2"
FocusMessagePane = "3"
FocusMemberPane = "4"
SelectNext = "j"
SelectPrevious = "k"
CycleFocusNext = { keys = ["tab", "l", "right"] }
CycleFocusPrevious = { keys = ["<S-tab>", "h", "left"] }
HalfPageDown = "<C-d>"
HalfPageUp = "<C-u>"
ScrollViewportDown = "J"
ScrollViewportUp = "K"
JumpTop = "gg"
JumpBottom = "G"
ScrollHorizontalLeft = "H"
ScrollHorizontalRight = "L"
ResizePaneLeft = { keys = ["<A-h>", "<A-left>"] }
ResizePaneRight = { keys = ["<A-l>", "<A-right>"] }
Quit = "q"
CopyMessage = "y"
ReactMessage = "r"
ReplyMessage = "R"
DeleteMessage = "d"
EditMessage = "e"
OpenMessageUrl = "o"
PlayMedia = "x"
ViewMessageAttachment = "v"
ToggleGuildPane = "<leader>1"
ToggleChannelPane = "<leader>2"
ToggleMemberPane = "<leader>4"
OpenFocusedPaneAction = "<leader>a"
OpenCurrentUserProfile = "<leader>p"
OpenOptions = "<leader>o"
OpenNotificationInbox = "<leader>n"
ChannelSwitcher = "<leader><leader>"
VoiceDeafen = "<leader>vd"
VoiceMute = "<leader>vm"
VoiceLeave = "<leader>vl"

[keymap.groups]
"<leader>v" = "Voice"

[keymap.guild_actions]
MarkAsRead = "m"
MuteServer = "u"
LeaveServer = "l"
FolderSettings = "r"

[keymap.channel_actions]
JoinVoice = "j"
LeaveVoice = "l"
ShowPinnedMessages = "p"
ShowThreads = "t"
MarkAsRead = "m"
MuteChannel = "u"

[keymap.message_actions]
CopyMessage = "y"
ReactMessage = "r"
ReplyMessage = "R"
DeleteMessage = "d"
EditMessage = "e"
OpenMessageUrl = "o"
RemoveMessageEmbeds = "D"
PlayMedia = "x"
ViewMessageAttachment = "v"
GoToReferencedMessage = "g"
ShowMessageProfile = "p"
PinMessage = "P"
OpenThread = "t"
ShowReactionUsers = "u"
OpenPollVotePicker = "c"

[keymap.member_actions]
ShowProfile = "p"

[keymap.thread_actions]
MarkAsRead = "m"
ToggleFollow = "f"
Close = "c"
Lock = "l"
Edit = "e"
CopyLink = "y"
ToggleMute = "u"
NotificationSettings = "n"
Pin = "P"
Delete = "d"
CopyId = "i"

[keymap.composer]
OpenEditor = "<C-e>"
PasteClipboard = "<C-v>"
InsertNewline = { keys = ["<C-j>", "<S-enter>", "<C-enter>", "<A-enter>"] }
Submit = "enter"
Close = "esc"
ClearInput = "<C-c>"
RemoveLastAttachment = "delete"
DeletePreviousChar = "backspace"
DeletePreviousWord = { keys = ["<A-backspace>", "<C-backspace>", "<C-w>"] }
MoveCursorUp = "up"
MoveCursorDown = "down"
MoveCursorWordLeft = "<C-left>"
MoveCursorLeft = "left"
MoveCursorWordRight = "<C-right>"
MoveCursorRight = "right"
MoveCursorHome = "home"
MoveCursorEnd = "end"
ToggleReplyPing = "<A-p>"
```

</details><br>

You can customize key bindings. Check the [Keymap options](./docs/keymap-options.md) for the config format, supported actions.

## Performance

Concord is designed to stay lightweight in normal terminal use. In observed
typical use, it usually uses about 20-40 MB of memory.

Image-heavy screens can temporarily use more memory because compressed image
bytes need to be decoded before they can be rendered in the terminal. When many
images are loaded, memory can briefly rise to around 100-200 MB while decoding
and then drop again as work completes and caches are pruned.

To keep resource usage bounded, Concord limits media work in several places:

- Attachment previews are downloaded with an 8 MiB per-preview cap.
- Up to 4 attachment previews are fetched at once.
- Up to 2 inline image previews are decoded at once.
- Inline image previews, avatars, and custom emoji use small LRU caches.
- Image preview requests prefer resized Discord proxy URLs sized for the
  terminal instead of original full-size media when possible.
- The preview quality preset can lower preview source dimensions or opt into
  original source images. It does not change avatar or custom emoji sizing.

Message history is also cached with a per-channel limit, so long-running
sessions do not keep every message in memory forever.

## FAQ

### Can my account be blocked?

Honestly, no.

There are some path that did trigger a account block:

- Trying to **create a new DM channel and send a message to an unknown user**(meaning there was no pre-existing DM created through the Discord client) can immediately block your account temporarily.
- Some features that requires a hCapcha challenge on Discord's side.

Other features have not caused blocks in my testing.

That said, Concord is not an official Discord client. Using unofficial clients, automated user accounts, or self-bots can violate Discord's TOS, so there is always some risk. Use it at your own discretion.

### Does Concord support CAPTCHA?

No. If Discord requires a CAPTCHA during login, use token login instead.

## Security

- By default, tokens are stored in the system keychain when available.
- On Linux, keychain storage uses Secret Service when a compatible service is available.
- The `CONCORD_TOKEN` environment variable lets you provide a token without writing it to disk. This avoids leaving plaintext on disk, but the token is visible in `/proc/<pid>/environ` and in process listings while Concord is running.
- In `credentials.store = "auto"`, Concord falls back to **plain text** credentials under Concord's state directory when keychain storage is unavailable. In `keychain` mode, Concord does not fall back to plain storage. Keep fallback credential files secure and do not share them. You can use a token from that file to log in to the official Discord client, so treat it like a password.
- On Unix, the fallback credential's parent directory is created with `0700` and the credential file with `0600` permissions.

## Contributing

Any issues, pull requests, and feedback are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

## License

Concord is licensed under GPL-3.0-only.
