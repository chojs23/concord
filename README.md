# Concord

<img width="1613" height="848" alt="concord - a feature-rich TUI client for
  Discord" src="./docs/example.png" />

Concord is a feature-rich TUI (terminal user interface) client for Discord, written in Rust with ratatui. Full Discord experience, right in your terminal.

## Installation

### Homebrew

```sh
brew install chojs23/tap/concord
```

### Cargo

```sh
cargo install concord
```

To install the latest unreleased version directly from the Git repository:

```sh
cargo install --git https://github.com/chojs23/concord
```

### Nix

Run without installing (requires flakes enabled):

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

Then reference it as `concord.packages.${system}.default` in your configuration.

A development shell with the pinned Rust toolchain and `rust-analyzer` is also
available:

```sh
nix develop github:chojs23/concord
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

You need the Rust stable toolchain and Cargo.

```sh
git clone https://github.com/chojs23/concord.git
cd concord
cargo build --release
```

The release binary is produced at:

```sh
target/release/concord
```

By default, source builds can join voice channels and decode received voice
audio, but they do not open local audio input or output devices. To build with
voice playback and gated microphone transmit, enable the optional
`voice-playback` feature:

```sh
cargo build --release --features voice-playback
```

Linux playback uses the system audio stack through `cpal`. You may need ALSA
development files when building from source:

```sh
sudo apt install libasound2-dev
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

Email and QR code logins may trigger a CAPTCHA challenge on Discord's side. We cannot solve that, so I strongly recommend using token authentication.

Tokens are saved under Concord's config directory in plain text. See the Security section below for details.

### Guilds & Channels

- Browse servers with guild folder grouping
- Navigate text channels, threads, and forum channels
- View and filter forum posts (active / archived)
- Load pinned messages per channel
- Open channel actions for pinned messages, thread lists, and mark-as-read
- Join and leave voice channels
- Highlight active voice speakers in voice channel participant rows
- Track unread messages and mention counts per channel
- Mute and unmute channels and servers

### Messaging

- Send, edit, and delete messages
- Reply to specific messages
- Upload files by copying them from your file manager and pasting them into the composer
- Upload images copied directly to the system clipboard when the terminal forwards the paste key
- Use @mention autocomplete while composing messages
- View full message history with pagination
- Rich content display (embeds, attachments, stickers, and mentions)
- Detect URLs in message bodies and markdown links, then open them in your default browser
- Direct message shortcuts for copy, reply, edit, delete, pin/unpin, reactions,
  image viewing, and profile lookup

#### Markdown Rendering

![Markdown rendering example](./docs/markdown-example.png)

Concord renders a practical subset of Discord-style Markdown in message bodies:

- Headings: `# H1`, `## H2`, `### H3`
- Quotes: `> quoted text`
- Bullets: `- item` and `* item`
- Inline styles: `**bold**`, `*italic*`, and `` `inline code` ``
- Fenced code blocks with optional language labels, rendered as compact boxes
- Raw URLs and markdown link destinations are underlined and can be opened from message actions

### Reactions & Polls

- View, add, and remove emoji reactions (Unicode and custom server emoji)
- Browse who reacted with a specific emoji
- View and vote on polls

### Media & Images

- Inline image previews directly in the terminal
- Avatar and custom emoji rendering
- Download attachments to your platform Downloads directory (`XDG_DOWNLOAD_DIR` on Linux)
- Large centered attachment viewer with navigation

Image rendering is powered by [ratatui-image](https://github.com/benjajaja/ratatui-image). On startup, Concord queries the terminal to detect the best available graphics protocol. Supported protocols:

- **Kitty Graphics Protocol** - Kitty, WezTerm, Ghostty, etc.
- **iTerm2 Inline Images** - iTerm2, WezTerm, mintty, etc.
- **Sixel** - foot, mlterm, xterm (if compiled with Sixel support), etc.
- **Halfblocks** (fallback) - works on any terminal, but uses block characters instead of true pixels.

If your terminal does not support any graphics protocol, images will be rendered as halfblock approximations. For the best experience, use a terminal that supports the Kitty or iTerm2 protocol.

You can toggle image viewing on or off in the configuration file. When image viewing is off, attachments and emojis will be shown as text placeholders.

### Members & Profiles

- Member list with grouping
- Presence indicators (Online, Idle, DND, Offline)
- User profile popups with guild-specific details

### Typing Indicators & Read State

- Live "user is typing..." indicators
- Unread message tracking with mention counts
- Mark server, channel as read

### Notifications

- Desktop notifications for Discord messages that pass your Discord
  notification settings
- Active channel notifications are suppressed so Concord does not notify for
  the conversation you are already viewing
- On macOS, Concord plays one explicit notification sound so focused terminal
  windows do not silently swallow audible alerts

### Navigation & Keyboard shortcuts

All default key settings in this section can be customized. See
[Keymap options](#keymap-options) for the config format and supported actions.

Concord has a four-pane layout like Discord.
**Guilds (1)**, **Channels (2)**, **Messages (3)**, **Members (4)**

With default vim-style navigation:

| Key                  | Action                                     |
| -------------------- | ------------------------------------------ |
| `1` `2` `3` `4`      | Focus pane                                 |
| `Tab` / `Shift+Tab`  | Cycle focus forward / backward             |
| `h` / `l`, `←` / `→` | Move focus left / right                    |
| `j` / `k`, `↑` / `↓` | Move down / up                             |
| `J`, `K` / `H`, `L`  | Scroll viewport                            |
| `Ctrl+d` / `Ctrl+u`  | Half-page scroll                           |
| `Alt+h/l/←/→`        | Resize focused pane width                  |
| `g` / `G`            | Jump or scroll to top / bottom             |
| `Enter`              | Open or activate the selected item         |
| `/`                  | Filter the focused Guilds or Channels pane |
| `Space`              | Open leader shortcut window                |
| `i`                  | Text insert mode                           |
| `Esc`                | Close popup, cancel mode, or go back       |
| `q`                  | Quit                                       |

#### Leader key

Press `Space` to open the leader shortcut window.

| Key sequence     | Action                            |
| ---------------- | --------------------------------- |
| `Space`, `1`     | Toggle the Servers pane           |
| `Space`, `2`     | Toggle the Channels pane          |
| `Space`, `4`     | Toggle the Members pane           |
| `Space`, `a`     | Open actions for the focused pane |
| `Space`, `o`     | Choose concord option category    |
| `Space`, `v`     | Voice command prefix              |
| `Space`, `Space` | Open the fuzzy channel switcher   |

#### Action menus

Focus a pane, then press `Space`, `a` to open actions for that pane. Action
shortcuts are shown inside the leader popup and only run when the action is
enabled. In the Messages pane, the selected message also supports direct
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
| `v`      | View attachment     | Open the selected message's attachment viewer               |
| `p`      | Profile             | Open the selected message author's profile                  |
| `P`      | Pin / unpin         | Open a pin or unpin confirmation for the selected message   |

If a message contains more than one detected URL, `o` opens a numbered URL picker inside the leader popup so you can choose which link to open.

Server actions:

| Shortcut | Action              | Description                                           |
| -------- | ------------------- | ----------------------------------------------------- |
| `m`      | Mark server as read | Mark all unread viewable channels in this server read |
| `u`      | Mute / unmute       | Toggle server notification mute                       |

Channel actions:

| Shortcut | Action               | Description                                  |
| -------- | -------------------- | -------------------------------------------- |
| `j`      | Join voice           | Join the selected voice channel              |
| `l`      | Leave voice          | Leave the current voice channel              |
| `p`      | Show pinned messages | Open the selected channel's pinned messages  |
| `t`      | Show threads         | List threads for the selected channel        |
| `m`      | Mark as read         | Mark the selected channel read               |
| `u`      | Mute / unmute        | Toggle channel or category notification mute |

Voice commands:

| Sequence          | Action       | Description                               |
| ----------------- | ------------ | ----------------------------------------- |
| `Space`, `v`, `d` | Deafen voice | Toggle Concord's Discord voice deaf state |
| `Space`, `v`, `m` | Mute voice   | Toggle Concord's Discord voice mute state |
| `Space`, `v`, `l` | Leave voice  | Leave the current Concord voice channel   |

When the attachment viewer is open, press `d` to download the current attachment directly.

Hidden side panes give their width back to Messages. Pressing a hidden pane's
number key directly shows and focuses it again.

#### Composer

You can paste copied files into the composer to attach them. Pending uploads
are shown above the input before sending.

| Shortcut                   | Action            | Description                                                      |
| -------------------------- | ----------------- | ---------------------------------------------------------------- |
| `Ctrl+v`                   | paste clipboard   | Attach copied files or images when present, otherwise paste text |
| `Ctrl+e`                   | open $EDITOR      | Open $EDITOR on the current draft for long editing               |
| `Ctrl+c`                   | clear             | Clear current draft                                              |
| `Ctrl+Left`/ `Ctrl+Right`  | Jump word         | Jump the cursor by word                                          |
| `Ctrl+Backspace`/ `Ctrl+w` | Delete word       | Delete the word before the cursor                                |
| `Delete`                   | Detach attachment | Removes the last pending attachment                              |

#### Mention picker

When the @mention picker is open, use `Up` / `Down`,
`Ctrl+p` / `Ctrl+n`, `Tab`, or `Enter` to choose a mention.

#### Emoji picker

Type `:` plus at least two letters, such as `:he`, to pick Unicode or server
emoji while writing a message.

To react from the composer, select a message, enter insert mode, then type
`+:`. The reaction picker opens for that message. Press `/` to search, Enter to
lock the filter, then Enter again or use a shown shortcut to react.

#### Bot commands

When the composer input starts with a slash `/`, the command suggestion popup

#### Mouse support

Mouse support is also available: click to focus or select rows, double-click to
open or activate items, and use the wheel to scroll panes and popups.

### Configuration

Concord options are stored under Concord's config directory. If
`XDG_CONFIG_HOME` is set, Concord uses `$XDG_CONFIG_HOME/concord/config.toml`
for app options and `$XDG_CONFIG_HOME/concord/keymap.toml` for key settings.
Otherwise it uses the platform config directory. The usual fallback is
`~/.config/concord/config.toml` and `~/.config/concord/keymap.toml` on Linux,
matching files under `~/Library/Application Support/concord/` on macOS, and the
roaming AppData config directory on Windows.

- Disable all image previews with one master switch
- Toggle inline image previews
- Set image preview quality for attachments, embeds, and the attachment viewer
- Toggle avatar display
- Toggle custom emoji rendering
- Toggle desktop notifications
- Set your Discord voice mute and deaf state
- Set microphone and received voice volume from 0 to 100
- Allow gated microphone transmit while joined from this Concord session and not
  self-muted

You can change these from the in-app Options menu, and Concord saves them back
to `config.toml`. Key settings are read from `keymap.toml`.

Example:

```toml
[display]
# Master switch that hides all image previews when true.
disable_image_preview = false

# Show user avatars next to messages and in profile views.
show_avatars = true

# Render inline image previews for attachments and embeds.
show_images = true

# Preview quality: efficient, balanced, high, or original.
image_preview_quality = "balanced"

# Render custom Discord emoji as images when possible.
show_custom_emoji = true

# Crop avatars into circles instead of showing square images.
circular_avatars = false

[notifications]
# Show desktop notifications for Discord messages that pass notification rules.
desktop_notifications = true

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

`image_preview_quality` supports these values:

- `efficient`: smaller preview requests to reduce bandwidth and memory use.
- `balanced`: default quality with bounded resource use.
- `high`: sharper resized previews using lossless quality.
- `original`: request the original source image for previews when possible.

This setting only applies to attachment, embed, and attachment viewer previews.
Avatars and custom emoji keep their separate small-image behavior.

`desktop_notifications` under `[notifications]` controls OS notifications for Discord messages that
pass Discord notification settings.

`microphone_volume` and `voice_output_volume` accept `0` to `100` percent
and default to `100`, which preserves the normal audio level. In Voice Options, select
sensitivity and press `h`/`l` to adjust by 1 dB or `H`/`L` to adjust by 10 dB.

#### Keymap options

Concord reads key settings from the `[keymap]` section in `keymap.toml`.

Example `keymap.toml`:

```toml
[keymap]
StartComposer = { keys = ["c"] }
ReplyMessage = "<leader>mr"
VoiceDeafen = "<leader>vd"
VoiceMute = "<leader>vm"
VoiceLeave = "<leader>vl"

[keymap.groups]
"<leader>v" = "Voice"

[keymap.channel_actions]
MuteChannel = { keys = ["x"], description = "mute channel" }

[keymap.message_actions]
OpenThread = { keys = ["t"], description = "open thread" }

[keymap.composer]
OpenEditor = "<C-o>"
DeletePreviousWord = "<A-backspace>"
```

There are five kinds of keymap settings:

| Config path                                                                                                 | What it controls                                                                            |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `[keymap] leader`                                                                                           | The key that opens the leader popup. Defaults to `Space`.                                   |
| `[keymap] <ActionName>`                                                                                     | Directly assignable UI actions such as `StartComposer`, `ChannelSwitcher`, and `VoiceMute`. |
| `[keymap.groups]`                                                                                           | Optional titles for prefix popups, such as naming `<leader>v` as `Voice`.                   |
| `[keymap.guild_actions]`, `[keymap.channel_actions]`, `[keymap.member_actions]`, `[keymap.message_actions]` | Shortcuts shown inside focused-pane action menus opened by `OpenFocusedPaneAction`.         |
| `[keymap.composer]`                                                                                         | Shortcuts used while the message composer is open, such as editor and cursor commands.      |

`[keymap]` action values can be either a string or an object with `keys` and an
optional `description`:

```toml
[keymap]
StartComposer = "<leader>e"
ChannelSwitcher = { keys = ["<C-w>f", "<leader><C-w>"], description = "find channel" }
```

`keys` accepts one sequence or a list of sequences. Modifier chords use
Vim-style angle syntax, such as `<C-f>`, `<S-tab>`, and `<A-backspace>`.
Leader sequences like `<leader><C-w>`, compact plain sequences like `fd`, and
general multi-key prefixes like `<C-w>f` are supported. Prefix sequences show a
[which-key.nvim](https://github.com/folke/which-key.nvim) style popup. For example, `fd` opens an `f` popup after `f`, then runs
the action after `d`.

```toml
[keymap.channel_actions]
MuteChannel = { keys = ["u", "<C-u>"], description = "mute channel" }
```

Composer action values under `[keymap.composer]` use the same string or object
shape, but each `keys` entry must be one key chord because composer commands run
immediately while text is being typed:

```toml
[keymap.composer]
OpenEditor = { keys = ["<C-o>"], description = "open editor" }
DeletePreviousWord = "<A-backspace>"
```

Direct `[keymap]` actions and `leader` cannot use reserved keys: `Enter`, `Esc`,
`Backspace`, `Delete`, or `Ctrl+c`. Invalid, reserved, or conflicting bindings
are ignored for that action. `[keymap.composer]` is separate and can remap those
editing keys.

##### Directly assignable actions

These action names can be assigned directly under `[keymap]`. Defaults
that start with `<leader>` are shown without the leading `Space` in the leader
popup. `OpenDisplayOptions`, `OpenNotificationOptions`, and `OpenVoiceOptions`
have contextual defaults inside the Options popup, so assign your own full
sequence if you want direct keys for them.

| Action name               | Default config                     | Action                                       |
| ------------------------- | ---------------------------------- | -------------------------------------------- |
| `StartComposer`           | `"i"`                              | Start the message composer.                  |
| `OpenPaneFilter`          | `"/"`                              | Open the focused pane filter.                |
| `FocusGuildPane`          | `"1"`                              | Show and focus the Servers pane.             |
| `FocusChannelPane`        | `"2"`                              | Show and focus the Channels pane.            |
| `FocusMessagePane`        | `"3"`                              | Focus the Messages pane.                     |
| `FocusMemberPane`         | `"4"`                              | Show and focus the Members pane.             |
| `CycleFocusNext`          | `["tab", "l", "right"]`            | Cycle focus forward.                         |
| `CycleFocusPrevious`      | `["<S-tab>", "h", "left"]`         | Cycle focus backward.                        |
| `HalfPageDown`            | `"<C-d>"`                          | Half-page down.                              |
| `HalfPageUp`              | `"<C-u>"`                          | Half-page up.                                |
| `JumpTop`                 | `"g"`                              | Jump to the top.                             |
| `JumpBottom`              | `"G"`                              | Jump to the bottom.                          |
| `ScrollHorizontalLeft`    | `"H"`                              | Scroll focused pane horizontally left.       |
| `ScrollHorizontalRight`   | `"L"`                              | Scroll focused pane horizontally right.      |
| `ResizePaneLeft`          | `["<A-h>", "<A-left>"]`           | Shrink the focused pane width.               |
| `ResizePaneRight`         | `["<A-l>", "<A-right>"]`          | Grow the focused pane width.                 |
| `Quit`                    | `"q"`                              | Quit Concord.                                |
| `CopyMessage`             | `"y"`                              | Copy selected message content.               |
| `ReactMessage`            | `"r"`                              | Add or remove a reaction.                    |
| `ReplyMessage`            | `"R"`                              | Start a reply.                               |
| `DeleteMessage`           | `"d"`                              | Open delete confirmation.                    |
| `EditMessage`             | `"e"`                              | Start editing the selected message.          |
| `OpenMessageUrl`          | `"o"`                              | Open the selected message URL.               |
| `ViewMessageAttachment`   | `"v"`                              | Open the selected message attachment viewer. |
| `ShowMessageProfile`      | `"p"`                              | Open the selected message author's profile.  |
| `PinMessage`              | `"P"`                              | Open pin or unpin confirmation.              |
| `ToggleGuildPane`         | `"<leader>1"`                      | Toggle the Servers pane.                     |
| `ToggleChannelPane`       | `"<leader>2"`                      | Toggle the Channels pane.                    |
| `ToggleMemberPane`        | `"<leader>4"`                      | Toggle the Members pane.                     |
| `OpenFocusedPaneAction`   | `"<leader>a"`                      | Open actions for the currently focused pane. |
| `OpenOptions`             | `"<leader>o"`                      | Open the options category picker.            |
| `ChannelSwitcher`         | `"<leader><leader>"`               | Open channel switcher.                       |
| `OpenDisplayOptions`      | Contextual `d` after `OpenOptions` | Open Display options.                        |
| `OpenNotificationOptions` | Contextual `n` after `OpenOptions` | Open Notification options.                   |
| `OpenVoiceOptions`        | Contextual `v` after `OpenOptions` | Open Voice options.                          |
| `VoiceDeafen`             | `"<leader>vd"`                     | Toggle voice deafen.                         |
| `VoiceMute`               | `"<leader>vm"`                     | Toggle voice mute.                           |
| `VoiceLeave`              | `"<leader>vl"`                     | Leave the current Concord voice channel.     |

##### Composer actions

These action names can be assigned under `[keymap.composer]`. Configured keys
replace that action's defaults. Any printable single character can be configured,
but that key will run the composer action instead of inserting text.

| Composer action        | Default config                            | Action                               |
| ---------------------- | ----------------------------------------- | ------------------------------------ |
| `OpenEditor`           | `"<C-e>"`                                 | Open the current draft in `$EDITOR`. |
| `PasteClipboard`       | `"<C-v>"`                                 | Request clipboard paste.             |
| `InsertNewline`        | `["<S-enter>", "<C-enter>", "<A-enter>"]` | Insert a newline.                    |
| `Submit`               | `"enter"`                                 | Submit the composer.                 |
| `Close`                | `"esc"`                                   | Close the composer.                  |
| `ClearInput`           | `"<C-c>"`                                 | Clear the composer input.            |
| `RemoveLastAttachment` | `"delete"`                                | Remove the last pending attachment.  |
| `DeletePreviousChar`   | `"backspace"`                             | Delete the previous character.       |
| `DeletePreviousWord`   | `["<C-backspace>", "<C-w>"]`              | Delete the word before the cursor.   |
| `MoveCursorUp`         | `"up"`                                    | Move the cursor up.                  |
| `MoveCursorDown`       | `"down"`                                  | Move the cursor down.                |
| `MoveCursorWordLeft`   | `"<C-left>"`                              | Move the cursor one word left.       |
| `MoveCursorLeft`       | `"left"`                                  | Move the cursor left.                |
| `MoveCursorWordRight`  | `"<C-right>"`                             | Move the cursor one word right.      |
| `MoveCursorRight`      | `"right"`                                 | Move the cursor right.               |
| `MoveCursorHome`       | `"home"`                                  | Move the cursor to the start.        |
| `MoveCursorEnd`        | `"end"`                                   | Move the cursor to the end.          |

##### Focused pane actions

`OpenFocusedPaneAction` opens the action menu for the pane that currently has
focus. Server, channel, member, and message pane actions can be configured in
scoped tables. Focused-pane action menus keep their scoped actions visible, and
actions that do not apply to the current selection are shown dimmed and disabled.

Server pane actions:

```toml
[keymap.guild_actions]
MarkAsRead = { keys = ["m"], description = "mark server as read" }
MuteServer = { keys = ["u"], description = "mute server" }
```

| Scoped action | Default | Action                                                                     |
| ------------- | ------- | -------------------------------------------------------------------------- |
| `MarkAsRead`  | `m`     | Mark all unread viewable channels in the selected server read.             |
| `MuteServer`  | `u`     | Mute or unmute the selected server. Also accepts `ToggleMute` as an alias. |

Channel pane actions:

```toml
[keymap.channel_actions]
JoinVoice = { keys = ["j"], description = "join voice" }
LeaveVoice = { keys = ["l"], description = "leave voice" }
ShowPinnedMessages = { keys = ["p"], description = "show pinned messages" }
ShowThreads = { keys = ["t"], description = "show threads" }
MarkAsRead = { keys = ["m"], description = "mark as read" }
MuteChannel = { keys = ["u"], description = "mute channel" }
```

| Scoped action        | Default | Action                                                                                      |
| -------------------- | ------- | ------------------------------------------------------------------------------------------- |
| `JoinVoice`          | `j`     | Join the selected voice channel.                                                            |
| `LeaveVoice`         | `l`     | Leave the current Concord voice channel.                                                    |
| `ShowPinnedMessages` | `p`     | Open the selected channel's pinned messages. Also accepts `LoadPinnedMessages` as an alias. |
| `ShowThreads`        | `t`     | List threads for the selected channel.                                                      |
| `MarkAsRead`         | `m`     | Mark the selected channel read.                                                             |
| `MuteChannel`        | `u`     | Mute or unmute the selected channel or category. Also accepts `ToggleMute` as an alias.     |

Member pane actions:

```toml
[keymap.member_actions]
ShowProfile = { keys = ["p"], description = "show profile" }
```

| Scoped action | Default | Action                              |
| ------------- | ------- | ----------------------------------- |
| `ShowProfile` | `p`     | Open the selected member's profile. |

Messages pane actions can be configured under `[keymap.message_actions]`. This
menu only contains message actions that do not already have a direct message
shortcut.

```toml
[keymap.message_actions]
OpenThread = "t"
ShowReactionUsers = "u"
OpenPollVotePicker = "c"
```

| Action label         | Default shortcut | When it appears                                      |
| -------------------- | ---------------- | ---------------------------------------------------- |
| `Open thread`        | `t`              | The selected message has a thread. Otherwise dimmed. |
| `Show reacted users` | `u`              | Reaction users can be shown. Otherwise dimmed.       |
| `Choose poll votes`  | `c`              | A non-finalized poll is selected. Otherwise dimmed.  |

Scoped action `description` changes the label shown in the action menu. Multiple
configured `keys` work as aliases when they are unique in the current action
menu, and the popup shows them together, such as `[x/u]`. If two actions in the
same menu use the same configured key, that key is ignored for both actions. If
an action has no unique configured key, it falls back to `1` through `9`, then
`0`.

<details>
<summary>Default keymap config</summary>

```toml
[keymap]
leader = "space"
StartComposer = "i"
OpenPaneFilter = "/"
FocusGuildPane = "1"
FocusChannelPane = "2"
FocusMessagePane = "3"
FocusMemberPane = "4"
CycleFocusNext = { keys = ["tab", "l", "right"] }
CycleFocusPrevious = { keys = ["<S-tab>", "h", "left"] }
HalfPageDown = "<C-d>"
HalfPageUp = "<C-u>"
JumpTop = "g"
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
ViewMessageAttachment = "v"
ShowMessageProfile = "p"
PinMessage = "P"
ToggleGuildPane = "<leader>1"
ToggleChannelPane = "<leader>2"
ToggleMemberPane = "<leader>4"
OpenFocusedPaneAction = "<leader>a"
OpenOptions = "<leader>o"
ChannelSwitcher = "<leader><leader>"
VoiceDeafen = "<leader>vd"
VoiceMute = "<leader>vm"
VoiceLeave = "<leader>vl"

[keymap.groups]
"<leader>v" = "Voice"

[keymap.guild_actions]
MarkAsRead = "m"
MuteServer = "u"

[keymap.channel_actions]
JoinVoice = "j"
LeaveVoice = "l"
ShowPinnedMessages = "p"
ShowThreads = "t"
MarkAsRead = "m"
MuteChannel = "u"

[keymap.member_actions]
ShowProfile = "p"

[keymap.message_actions]
OpenThread = "t"
ShowReactionUsers = "u"
OpenPollVotePicker = "c"

[keymap.composer]
OpenEditor = "<C-e>"
PasteClipboard = "<C-v>"
InsertNewline = { keys = ["<S-enter>", "<C-enter>", "<A-enter>"] }
Submit = "enter"
Close = "esc"
ClearInput = "<C-c>"
RemoveLastAttachment = "delete"
DeletePreviousChar = "backspace"
DeletePreviousWord = { keys = ["<C-backspace>", "<C-w>"] }
MoveCursorUp = "up"
MoveCursorDown = "down"
MoveCursorWordLeft = "<C-left>"
MoveCursorLeft = "left"
MoveCursorWordRight = "<C-right>"
MoveCursorRight = "right"
MoveCursorHome = "home"
MoveCursorEnd = "end"
```

</details>

## Performance

Concord is designed to stay lightweight in normal terminal use. In observed
typical use, it usually uses about 20-40 MB of memory.

Image-heavy screens can temporarily use more memory because compressed image
bytes need to be decoded before they can be rendered in the terminal. When many
images are loaded, memory can briefly rise to around 100-200 MB while decoding
and then drop again as work completes and caches are pruned.

To keep resource usage bounded, Concord limits media work in several places:

- Attachment previews are downloaded with an 8 MiB per-preview cap.
- Attachment downloads are capped at 64 MiB.
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

- Tokens are stored as **plain text** in Concord's config directory. So keep that file secure and do not share it. You can use the token from that file to log in to the official Discord client, so treat it like a password.
- On Unix, the credential's parent directory is created with `0700` and the credential file with `0600` permissions.
- All concord state (config, keymap, credential, log) lives under a single `concord/` directory inside `XDG_CONFIG_HOME` when it is set, or inside the platform config directory otherwise.
- No system keychain integration yet.

## Contributing

Any issues, pull requests, and feedback are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

## License

Concord is licensed under GPL-3.0-only.
