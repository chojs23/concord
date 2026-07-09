# Theme options

Concord reads colors from the `[theme]` section in `theme.toml`, stored
alongside `config.toml` and `keymap.toml`.

Example `theme.toml`:

```toml
[theme]
accent = "#39ff14"
background = "#1a1a2e"
error = "#ff5555"
```

Every field is optional. A field left out — or set to something that isn't a
valid `"#rrggbb"` hex string — falls back to its built-in default. An invalid
(but present) value logs a warning at startup instead of failing to start;
run `concord --check-config` to see those warnings without launching the
full app.

See [`theme.toml.example`](../theme.toml.example) in the repo root for a
copy of this file with every field already filled in at its default value,
ready to edit and drop in.

## Fields that adapt to your terminal

Most fields default to a fixed RGB value and behave identically everywhere.
A handful instead default to your terminal's own ANSI color, which means
they currently look different across terminals/terminal themes on purpose:

`accent`, `dim`, `border`, `text`, `active`, `success`, `warning`, `error`,
`info`, `self_reaction`, `mention_role_fallback`, `dm_icon`

Setting any of these explicitly switches that field from "matches whatever
your terminal calls that color" to "always this exact truecolor value,"
regardless of terminal theme. That's a real behavior change, not just
picking a prettier shade of the same thing.

Two fields (`panel_title`, `unread_bright`) default to your terminal's
reset/default foreground color specifically, which has no hex equivalent at
all — they can still be set to a fixed hex, they just don't have one by
default.

## Field reference

| Field | Default | What it controls |
| --- | --- | --- |
| `text` | terminal white | Main body/foreground text color. |
| `accent` | terminal cyan | Focused pane borders, selection markers, tag chips, links. |
| `dim` | terminal dark gray | Muted/secondary text: scrollbar track, timestamps, hints, unfocused labels. |
| `border` | terminal dark gray | Unfocused pane border color. |
| `background` | `#183641` | Panel/highlighted-row background, e.g. the selected member row. |
| `selection_bg` | `#282D5A` | Selection/highlight background behind mention pickers and dropdowns. |
| `success` | terminal green | Success toasts, online presence dot, accepted friend requests, selected message/forum-post borders. |
| `warning` | terminal yellow | Warning badges, muted/deafened icons, editing labels, pending friend requests, failed image-preview notices. |
| `error` | terminal red | Error toasts, delete confirmations, blocked users, gateway connection issues, DnD presence dot. |
| `info` | terminal blue | Toast info messages, embed titles and links. |
| `blurple` | `#5865F2` | Discord's brand blurple: unread-message banner, bot badges, slash-command highlighting. |
| `mention` | `#FFA500` | Discord's "you were mentioned" orange: mention badges in the channel list, and inline code spans in message bodies. |
| `unread_badge` | `#ED4245` | Discord-style unread-divider red, distinct from `error`. |
| `mention_self_bg` | `#5C4C23` | Background behind message text where you were @mentioned. |
| `mention_other_bg` | `#28325C` | Background behind message text where someone else was @mentioned. |
| `mention_other_fg` | `#C1CEF7` | Foreground for someone-else-was-mentioned highlighted text. |
| `self_reaction` | terminal yellow | Highlight for your own reactions, and the self-mention text-highlight foreground. |
| `read_dim` | `#828282` | Dim tone for read/seen channels in the channel list. Fixed RGB rather than ANSI dim, so wide CJK glyphs dim as evenly as ASCII. |
| `unread_bright` | terminal reset | Bright marker for unread/notified channels. |
| `scrollbar_thumb` | `#AAAAAA` | Scrollbar thumb color. |
| `active` | terminal green | "Active"/highlighted text state, e.g. the active voice channel indicator. |
| `panel_title` | terminal reset | Panel title text color. |
| `selected_forum_post_border` | terminal green | Selected forum-post card border. |
| `selected_message_border` | terminal green | Selected message card border. |
| `presence_idle` | `#B48C00` | Idle presence dot color. |
| `mention_role_fallback` | terminal magenta | Fallback color for @role/@everyone mention chips with no role color set. |
| `dm_icon` | terminal magenta | Direct Messages entry icon/text color in the server list. |

Colors that come directly from Discord — a member's role color, a folder
color, an embed's accent color — are not themeable, since they're
per-server/per-message data rather than app styling. Only the *fallback*
used when Discord doesn't supply one (e.g. `text` for an uncolored role) is
configurable, and it's covered by the general field it reuses rather than a
separate theme field.
