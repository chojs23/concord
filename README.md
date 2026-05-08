# Concord

Concord is a terminal user interface client for Discord, written in Rust.

It connects to Discord through the gateway and REST APIs, renders a keyboard and mouse driven terminal UI, and keeps local Discord state in sync through an event driven app loop.

## Features

- Terminal UI built with `ratatui` and `crossterm`.
- Discord gateway and REST integration for live events and user actions.
- QR login and password login flows, including MFA handling.
- Local token storage at `~/.concord/credential`.
- Guild, channel, DM, thread, forum post, member, presence, and typing state handling.
- Message actions for loading history, sending, editing, deleting, replying, pinning, acknowledging, and opening related URLs.
- Reactions, reaction user loading, poll voting, user profile loading, pinned messages, and attachment preview/download support.
- Media-oriented UI support through avatar, emoji, image, and attachment preview caches.

## Install

### Homebrew

```sh
brew install chojs23/tap/concord
```

### Cargo

```sh
cargo install --git https://github.com/chojs23/concord
```

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

## Authentication

Concord supports three login methods:

- Paste an existing Discord token.
- Login with email or phone and password. MFA is supported when Discord requires it.
- Login with a QR code by scanning it from the Discord mobile app.

On startup, Concord first tries to load an existing token from `~/.concord/credential`. If no saved token is available, it opens the login flow. After a successful login, the token is saved back to the same path.

Warning: the token is currently stored locally as plain text. On Unix systems, Concord creates the config directory with `0700` permissions and writes the credential file with `0600` permissions, but the token is not encrypted or stored in a system keychain.

Email/password login and QR login can fail if Discord requires CAPTCHA. Concord does not support solving CAPTCHA in the terminal. If that happens, use token login instead.

## FAQ

### Can my account be blocked?

In day-to-day use, I have not seen an account block after several months of using Concord.
There was one path that did trigger a temporary block: trying to create new DM channel and send a message to an unknown user immediately blocked my account for 30 minutes. That feature has been removed. Other supported features have not caused blocks in my testing.

That said, Concord is not an official Discord client. Using unofficial clients, automated user accounts, or self-bots can violate Discord's Terms of Service, so there is always some risk. Use it at your own discretion.

## License

Concord is licensed under GPL-3.0-only.
