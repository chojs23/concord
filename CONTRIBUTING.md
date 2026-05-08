# Contributing to Concord

Thanks for your interest in Concord. Any issues, pull requests, and feedback are welcome.

## Ground rules

- Concord is an **unofficial** Discord client. Using it can violate Discord's
  Terms of Service. Do not contribute features that automate self-bot behavior,
  mass actions, or scraping that would put a real user account at risk.
- Concord is licensed under **GPL-3.0-only**. By submitting a contribution you
  agree it will be distributed under the same license.
- Be kind and concrete. Bug reports and feature proposals are easier to act on
  when they include a reproduction or a clear use case.

## Reporting bugs

Open an issue at <https://github.com/chojs23/concord/issues> with:

- OS, terminal emulator, and graphics protocol you expect (Kitty, iTerm2,
  Sixel, Halfblocks).
- Concord version
- Steps to reproduce, expected behavior, actual behavior.
- Relevant log excerpts. You can enable file logging via the in-app diagnostics
  popup; **redact tokens, passwords, MFA codes, QR tickets, and any raw auth
  response bodies before sharing logs**.

## Development setup

You need the Rust **stable** toolchain. MSRV is **1.85** and the edition is
**2024**.

```sh
git clone https://github.com/chojs23/concord.git
cd concord
cargo build
cargo run
```

A Nix dev shell with the pinned toolchain and `rust-analyzer` is available:

```sh
nix develop
```

Cargo is the only build tool. There is no Makefile, Justfile, or workspace
root.

## Before you push

Run these locally; CI runs the same checks.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

For release-shaped builds:

```sh
cargo build --release
```

## Commits and pull requests

- Use semantic commit subjects: `feat:`, `fix:`, `refactor:`, `docs:`,
  `chore:`, `test:`. Keep the subject under ~72 characters.
- Wrap commit-message prose at standard git widths. Put short identifiers in
  backticks; put multi-line code in indented fenced blocks.
- Explain non-obvious trade-offs in the body. The "why" matters more than the
  "what".
- One logical change per PR is easier to review than a grab bag.
- Fill in the PR description with: what the change does, why, how you tested
  it, and screenshots or recordings for UI changes if you can.
