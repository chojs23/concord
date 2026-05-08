# Concord

Concord is a terminal user interface client for Discord. Full Discord experience, right in your terminal.

<img width="1613" height="848" alt="concord" src="./docs/example.png" />

## Features

### Authentication

- **Token** : paste an existing Discord token.
- **Email / Password** : login with credentials. MFA (TOTP, SMS) is fully supported.
- **QR Code** : scan the code from the Discord mobile app.

Tokens are saved to `~/.concord/credential` in plain text. See the Security section below for details.

### Guilds & Channels

- Browse servers with guild folder grouping
- Navigate text channels, threads, and forum channels
- View and filter forum posts (active / archived)
- Load pinned messages per channel
- Track unread messages and mention counts per channel

### Messaging

- Send, edit, and delete messages
- Reply to specific messages
- View full message history with pagination
- Rich content display(embeds, attachments, stickers, and mentions)

### Reactions & Polls

- View, add, and remove emoji reactions (Unicode and custom server emoji)
- Browse who reacted with a specific emoji
- View and vote on polls

### Media & Images

- Inline image previews directly in the terminal
- Avatar and custom emoji rendering
- Download attachments to `~/Downloads`
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
- Mark channels as read

### Navigation & Keybindings

Concord has Four-pane layout like discord.
**Guilds (1)**, **Channels (2)**, **Messages (3)**, **Members (4)**

With vim-style navigation:

| Key                 | Action           |
| ------------------- | ---------------- |
| `1` `2` `3` `4`     | Focus pane       |
| `j` / `k`           | Move down / up   |
| `J`, `K` / `H`,`L`  | Scroll viewport  |
| `Ctrl+d` / `Ctrl+u` | Half-page scroll |
| `i`                 | Text insert mode |
| `a`                 | Action menu      |
| `o`                 | Options menu     |
| `q` / `Ctrl+c`      | Quit             |

Full mouse support is also available.

### Configuration

Display options are stored in `~/.concord/config.toml`:

- Toggle inline image previews
- Toggle avatar display
- Toggle custom emoji rendering

## Install

### Homebrew

```sh
brew install chojs23/tap/concord
```

### Cargo

```sh
cargo install --git https://github.com/chojs23/concord
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

## FAQ

### Can my account be blocked?

In day-to-day use, I have not seen an account block after several months of using Concord.
There was one path that did trigger a temporary block: trying to create new DM channel and send a message to an unknown user immediately blocked my account for 30 minutes. That feature has been removed. Other supported features have not caused blocks in my testing.

That said, Concord is not an official Discord client. Using unofficial clients, automated user accounts, or self-bots can violate Discord's Terms of Service, so there is always some risk. Use it at your own discretion.

### Does Concord support CAPTCHA?

No. If Discord requires a CAPTCHA during login, use token login instead.

## Security

- Tokens are stored as **plain text** in `~/.concord/credential`. So keep that file secure and do not share it. You can use the token from that file to log in to the official Discord client, so treat it like a password.
- On Unix, the config directory is created with `0700` and the credential file with `0600` permissions.
- No system keychain integration yet.

## License

Concord is licensed under GPL-3.0-only.
