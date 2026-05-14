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

The installer places `concord` under `$CARGO_HOME/bin`, which is usually
`~/.cargo/bin`. Make sure that directory is on your `PATH` before running
`concord`.

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

## Features

Concord does not currently support voice calls, but will be added.
For now it is focusing ui/ux and conveniency features.

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
- Track unread messages and mention counts per channel
- Mute and unmute channels and servers

### Messaging

- Send, edit, and delete messages
- Reply to specific messages
- Upload files by copying them from your file manager and pasting them into the composer
- Use @mention autocomplete while composing messages
- View full message history with pagination
- Rich content display (embeds, attachments, stickers, and mentions)
- Message action menu for reply, edit, delete, open thread, show profile,
  pin/unpin, reactions, poll votes, and attachment/image actions

### Reactions & Polls

- View, add, and remove emoji reactions (Unicode and custom server emoji)
- Browse who reacted with a specific emoji
- View and vote on polls

### Media & Images

- Inline image previews directly in the terminal
- Avatar and custom emoji rendering
- Download attachments to your platform Downloads directory (`XDG_DOWNLOAD_DIR` on Linux)
- Full-screen image viewer with navigation

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

### Navigation & Keybindings

Concord has a four-pane layout like Discord.
**Guilds (1)**, **Channels (2)**, **Messages (3)**, **Members (4)**

With vim-style navigation:

| Key                       | Action                               |
| ------------------------- | ------------------------------------ |
| `1` `2` `3` `4`           | Focus pane                           |
| `Tab` / `Shift+Tab`       | Cycle focus forward / backward       |
| `j` / `k`, arrows         | Move down / up                       |
| `J`, `K` / `H`, `L`       | Scroll viewport                      |
| `Ctrl+d` / `Ctrl+u`       | Half-page scroll                     |
| `Alt+h/l/←/→`             | Resize focused pane width            |
| `g` / `G`, `Home` / `End` | Jump or scroll to top / bottom       |
| `Enter`                   | Open or activate the selected item   |
| `Space`                   | Open leader shortcut window          |
| `i`                       | Text insert mode                     |
| `Esc`                     | Close popup, cancel mode, or go back |
| `q`                       | Quit                                 |

#### Leader key

Press `Space` to open the leader shortcut window.

| Key sequence     | Action                            |
| ---------------- | --------------------------------- |
| `Space`, `1`     | Toggle the Servers pane           |
| `Space`, `2`     | Toggle the Channels pane          |
| `Space`, `4`     | Toggle the Members pane           |
| `Space`, `a`     | Open actions for the focused pane |
| `Space`, `o`     | Open concord options              |
| `Space`, `Space` | Open the fuzzy channel switcher   |

#### Action menus

Focus a pane, then press `Space`, `a` to open actions for that pane. Action
shortcuts are shown inside the leader popup and only run when the action is
enabled. Messages also support a direct message action menu from the selected
message.

Server actions:

| Shortcut | Action              | Description                                           |
| -------- | ------------------- | ----------------------------------------------------- |
| `m`      | Mark server as read | Mark all unread viewable channels in this server read |

Channel actions:

| Shortcut | Action               | Description                                 |
| -------- | -------------------- | ------------------------------------------- |
| `p`      | Show pinned messages | Open the selected channel's pinned messages |
| `t`      | Show threads         | List threads for the selected channel       |
| `m`      | Mark as read         | Mark the selected channel read              |

When the image viewer is open, press `d` to download the current image directly.

Hidden side panes give their width back to Messages. Pressing a hidden pane's
number key directly shows and focuses it again.

#### Composer

You can paste copied files into the composer to attach them. Pending uploads
are shown above the input before sending, and `Ctrl+Backspace` removes the last
pending attachment.

Use `Ctrl+Left` / `Ctrl+Right` to jump the cursor by word.
Use `Ctrl+c` to clear the current draft.

#### Mention picker

When the @mention picker is open, use `Up` / `Down`,
`Ctrl+p` / `Ctrl+n`, `Tab`, or `Enter` to choose a mention.

#### Emoji picker

Type `:` plus at least two emoji shortcode letters, such as `:he`, to open
Unicode emoji and current-server custom emoji suggestions. Use `Up` / `Down`,
`Ctrl+p` / `Ctrl+n`, `Tab`, or `Enter` to choose an emoji. Complete Unicode
shortcodes such as `:heart:` are converted to their emoji when the message is
sent; selected custom emojis are sent using Discord's custom emoji markup.

#### Mouse support

Mouse support is also available: click to focus or select rows, double-click to
open or activate items, and use the wheel to scroll panes and popups.

### Configuration

Display options are stored under Concord's config directory. If
`XDG_CONFIG_HOME` is set, Concord uses
`$XDG_CONFIG_HOME/concord/config.toml`. Otherwise it uses the platform config
directory. The usual fallback is `~/.config/concord/config.toml` on Linux,
`~/Library/Application Support/concord/config.toml` on macOS, and the roaming
AppData config directory on Windows.

- Disable all image previews with one master switch
- Toggle inline image previews
- Set image preview quality for attachments, embeds, and the image viewer
- Toggle avatar display
- Toggle custom emoji rendering
- Toggle desktop notifications

You can change these from the in-app Options menu, and Concord saves them back
to the config file.

Example:

```toml
[display]
disable_image_preview = false
show_avatars = true
show_images = true
image_preview_quality = "balanced"
show_custom_emoji = true
desktop_notifications = true
```

`image_preview_quality` supports these values:

- `efficient`: smaller preview requests to reduce bandwidth and memory use.
- `balanced`: default quality with bounded resource use.
- `high`: sharper resized previews using lossless quality.
- `original`: request the original source image for previews when possible.

This setting only applies to attachment, embed, and image viewer previews.
Avatars and custom emoji keep their separate small-image behavior.

`desktop_notifications` controls OS notifications for Discord messages that
pass Discord notification settings. On macOS, Concord keeps the visual
notification and audible alert separate to avoid duplicate sounds while still
playing a sound when the terminal app is focused.

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

In day-to-day use, I have not seen an account block after several months of using Concord.
There was one path that did trigger a temporary block: trying to **create a new DM channel and send a message to an unknown user**(meaning there was no pre-existing DM created through the Discord client) immediately blocked my account for 30 minutes. That feature has been removed. Other supported features have not caused blocks in my testing.

That said, Concord is not an official Discord client. Using unofficial clients, automated user accounts, or self-bots can violate Discord's TOS, so there is always some risk. Use it at your own discretion.

### Does Concord support CAPTCHA?

No. If Discord requires a CAPTCHA during login, use token login instead.

## Security

- Tokens are stored as **plain text** in Concord's config directory. So keep that file secure and do not share it. You can use the token from that file to log in to the official Discord client, so treat it like a password.
- On Unix, the credential's parent directory is created with `0700` and the credential file with `0600` permissions.
- All concord state (config, credential, log) lives under a single `concord/` directory inside `XDG_CONFIG_HOME` when it is set, or inside the platform config directory otherwise.
- No system keychain integration yet.

## Contributing

Any issues, pull requests, and feedback are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

## License

Concord is licensed under GPL-3.0-only.
