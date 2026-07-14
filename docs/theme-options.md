# Theme options

Concord reads `theme.toml` beside `config.toml` and `keymap.toml`. Named UI
Highlight Groups control colors and text modifiers, while `[ui.border]`
controls border shapes. Every group and field is optional.

```toml
[highlight.Normal]
foreground = "terminal_default"
background = "terminal_default"

[highlight.FocusBorder]
foreground = "light_magenta"

[highlight.ComposerPickerBorder]
link = "FocusBorder"

[highlight.Selection]
background = "none"

[ui.border]
default = "plain"
composer = "rounded"
```

## Group fields

| Field | Type | Meaning |
| --- | --- | --- |
| `link` | group name or `"none"` | Inherit unset fields from another known group, or detach the built-in link. |
| `foreground` | color or `"none"` | Text, marker, or border color; `none` clears the foreground after inheritance. |
| `background` | color or `"none"` | Cell background color; `none` clears the background after inheritance. |
| `bold` | boolean | Enable or disable bold. |
| `italic` | boolean | Enable or disable italic. |
| `dim` | boolean | Enable or disable dim. |
| `underline` | boolean | Enable or disable underline. |
| `strikethrough` | boolean | Enable or disable strikethrough. |

A child inherits only fields it does not set. For example,
`FocusedPaneBorder` inherits the foreground from `FocusBorder` and adds bold.
Setting `bold = false` explicitly removes inherited bold. A direct field on a
child wins over the linked group. `foreground = "none"` and
`background = "none"` clear that channel after inheritance.

Links may point forward or backward. Link cycles warn and ignore cyclic
inheritance instead of stopping startup. Group and field names are exact and
case-sensitive. Unknown groups, unknown fields, wrong field types, invalid
colors, invalid border shapes, and invalid links warn and are ignored one leaf
at a time. Valid sibling fields still apply. Run `concord --check-config` to
see warnings.

## Colors

Supported values are `none`, `terminal_default`, the 16 canonical ANSI names,
or a six-digit RGB value with an optional `#`.

`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`,
`dark_gray`, `light_red`, `light_green`, `light_yellow`, `light_blue`,
`light_magenta`, `light_cyan`, `white`

ANSI values use the terminal palette. `none` removes the color channel so the
style does not overwrite the cell underneath. `terminal_default` explicitly
overwrites it with the terminal's default color. Aliases such as `reset`,
`darkgray`, and `bright_red` are not accepted.

## Border shapes

Border shape is widget geometry, so it is configured separately from
Highlight Group colors and modifiers.

```toml
[ui.border]
default = "plain"
pane = "double"
composer = "rounded"
modal = "plain"
picker = "plain"
login = "plain"
message = "rounded"
forum = "rounded"
```

| Field | Built-in default | What it controls |
| --- | --- | --- |
| `default` | `plain` | Fallback for every omitted surface. |
| `pane` | `default` | Main pane frames. |
| `composer` | `rounded` | Composer frame. |
| `modal` | `default` | Modal popup frames. |
| `picker` | `default` | Composer mention, command, and emoji picker frames. |
| `login` | `default` | Login input and QR frames. |
| `message` | `rounded` | Selected message cards. |
| `forum` | `rounded` | Forum post cards. |

Supported shapes are `plain`, `rounded`, `double`, `thick`,
`light_double_dashed`, `heavy_double_dashed`, `light_triple_dashed`,
`heavy_triple_dashed`, `light_quadruple_dashed`,
`heavy_quadruple_dashed`, `quadrant_inside`, and `quadrant_outside`. When
`[ui.border]` is absent, Concord preserves its built-in rounded Composer,
message cards, and forum cards, with plain remaining surfaces. When `default`
is set, every omitted surface uses that value. A surface field overrides it.

Border sides remain fixed. Allowing a theme to remove a side would change the
inner widget area and break layout calculations. Message and forum cards use
the same Ratatui glyph sets as block borders, so their configured shape stays
consistent with the other surfaces.

## Common semantic groups

These groups describe reusable text meaning. Component groups link to them, so
one parent override can update related surfaces without coupling render code to
a specific color or modifier.

| Group | Built-in style | What it controls |
| --- | --- | --- |
| `Strong` | bold | General strong emphasis. |
| `Emphasis` | italic | General prose emphasis. |
| `Muted` | dim | Low-emphasis content. |
| `Title` | `Strong` | Parent style for titles. |
| `Heading` | `Strong` | Parent style for section headings. |
| `Decoration` | `Muted` | Tree branches, dividers, and separator glyphs. |
| `Hint` | `Muted` | Guidance, status notices, and overflow summaries. |
| `Description` | `Muted` | Secondary descriptions and inactive detail values. |
| `Shortcut` | `Muted` | Keyboard shortcuts shown beside actions. |
| `Activity` | `Muted` | Member and profile activity details. |
| `ChannelTypeMarker` | `Muted` | Channel and thread type markers, including group-DM and populated-voice markers. |
| `FieldLabel` | `Muted` | Labels for detail and filter fields. |
| `SearchContext` | `Muted` | Parent locations and identifiers appended to search results. |
| `Timestamp` | `Muted` | Parent style for timestamps. |
| `Placeholder` | `Muted` | Empty-state and placeholder text. |
| `Disabled` | `Muted` | Disabled controls and values. |
| `Loading` | `Muted` | Loading-state text. |
| `Edited` | `Muted`, italic | Edited markers. |
| `Unavailable` | `Muted`, strikethrough | Unavailable content. |

## Linked UI groups

These component groups make broad and narrow customization work together. The
link shown is the built-in default.

| Group | Default link and additions | What it controls |
| --- | --- | --- |
| `LoginTitle` | `Title`, cyan foreground | Login screen title. |
| `LoginHint` | `Muted` | Login hints and secondary instructions. |
| `PaneTitle` | `Title` | Main pane titles. |
| `ModalTitle` | `Title` | Popup titles. |
| `ComposerTitle` | `Title` | Composer title. |
| `HeaderTitle` | `Title`, cyan foreground | Header title. |
| `HeaderLabel` | `Muted` | Header labels such as `Connected as` and `Voice`. |
| `MessageAuthor` | `Strong` | Message author names. |
| `MessageTimestamp` | `Timestamp` | Message timestamps. |
| `CategoryHeading` | `Heading` | Channel category headings. |
| `MemberGroupHeading` | `Heading` | Complete member group headings, including counts. |
| `MessageSecondary` | `Muted` | Typing status, reply previews, and secondary system-message content. |
| `ForumSecondary` | `Muted` | Forum age, archive, lock, and empty-activity details. |
| `MarkdownHeading1` | `Heading`, cyan foreground | Level-one Markdown headings. |
| `MarkdownHeading2` | `Heading`, underline | Level-two Markdown headings. |
| `MarkdownHeading3` | `Heading` | Level-three Markdown headings. |
| `EmbedAuthor` | `Emphasis` | Embed author line. |
| `EmbedTitle` | `Strong`, blue foreground | Embed title. |
| `EmbedFieldName` | `Strong`, underline | Embed field names. |
| `EmbedFooter` | `Muted`, italic | Embed footer. |
| `CodeBlockBorder` | `Border`, dim | Fenced-code block border. Code text remains controlled by Syntect. |
| `ScrollbarTrack` | `ScrollbarThumb`, dim | Scrollbar track. |
| `UnavailableEmoji` | `Unavailable` | Unavailable emoji in Composer pickers. |
| `HeaderError` | `Error`, bold | Header error state. |
| `HeaderWarning` | `Warning`, bold | Header warning state. |
| `PaneBorder` | `Border` | Unfocused main pane borders. |
| `FocusedPaneBorder` | `FocusBorder`, bold | Focused main pane border. |
| `LoginBorder` | `FocusBorder` | Login input and QR frames. |
| `ComposerBorder` | `Border` | Inactive Composer border. |
| `ActiveComposerBorder` | `FocusBorder`, bold | Active Composer border. |
| `ModalBorder` | `FocusBorder`, bold | Modal popup frames. |
| `ComposerPickerBorder` | `FocusBorder` | Mention, command, and emoji picker frames. |
| `SelectedRow` | `Selection` | Selected rows and picker entries. Set a background on this group or `Selection` for filled selection. |
| `SelectionMarker` | `Selection` | Selection arrows in list panes and pickers, including the member pane. |
| `ActiveField` | cyan foreground, bold | Active form fields, filters, and popup commands without a filled background. |
| `ActiveTab` | `Selection` | Active popup tabs. |
| `MessageSelectedBorder` | `SelectionBorder` | Selected message frame. |
| `ForumBorder` | `FocusBorder` | Unselected forum-post frames. |
| `ForumSelectedBorder` | `SelectionBorder` | Selected forum-post frame and marker. |
| `ImageOverflow` | `MessageAttachment`, bold | Additional-image count labels. |
| `BotBadge` | `Normal`, `#5865F2` background, bold | Message bot badge. |
| `PresenceOffline` | `Normal`, dim | Offline and unknown presence. |

Changing a parent updates every child that remains linked. Override a child to
change one component only. Use `link = "none"` when a child must stop inheriting
from its built-in parent.

## Base and content groups

| Group | Built-in style | What it controls |
| --- | --- | --- |
| `Normal` | terminal foreground and background | Application canvas, normal fills, and default text base. |
| `Border` | dark gray foreground | General structural borders. |
| `FocusBorder` | cyan foreground | Parent color for focused surfaces. |
| `Selection` | cyan foreground, no background, bold, not dim | Parent style for selected content. |
| `SelectionBorder` | green foreground, bold | Parent style for framed selection. |
| `ScrollbarThumb` | `#AAAAAA` foreground | Scrollbar thumbs. |
| `UnreadNotice` | cyan foreground, bold | New-message notices and forum counts. |
| `Editing` | yellow foreground | Fields while editing. |
| `Reaction` | cyan foreground | Reactions not made by the current user. |
| `SelfReaction` | yellow foreground | Current-user reactions. |
| `PresenceOnline` | green foreground | Online presence. |
| `PresenceIdle` | `#B48C00` foreground | Idle presence. |
| `PresenceDnd` | red foreground | Do Not Disturb presence. |
| `VoiceDisabled` | yellow foreground | Self-muted and self-deafened header state. |
| `VoiceConnection` | yellow foreground, bold | Active voice connection header state. |
| `FolderFallback` | cyan foreground | Server folder without a Discord color. |
| `NavigationActive` | green foreground, bold | Currently open navigation destination. |
| `NavigationMentioned` | `#FFA500` foreground | Mentioned navigation destination. |
| `NavigationNotified` | terminal foreground | Notified navigation destination. |
| `NavigationUnread` | terminal foreground | Unread navigation destination. |
| `MentionBadge` | `#FFA500` foreground | Navigation mention counts. |
| `NotificationBadge` | terminal foreground | Notification and DM unread counts. |
| `JoinedVoiceChannel` | yellow foreground, bold | Joined voice channel. |
| `VoiceSpeaking` | green foreground, bold | Active voice speaker. |
| `ReplyPingEnabled` | cyan foreground | Enabled reply-ping state. |
| `Tag` | cyan foreground | Forum and picker tags. |
| `RelationshipFriend` | green foreground | Friend state. |
| `RelationshipIncoming` | yellow foreground | Incoming friend request. |
| `RelationshipOutgoing` | yellow foreground | Outgoing friend request. |
| `RelationshipBlocked` | red foreground | Blocked relationship. |
| `RelationshipNone` | terminal foreground, dim | No relationship. |
| `GaugeFill` | cyan foreground | Filled option gauges. |
| `MessageBody` | terminal foreground | Normal message content and emoji fallback. |
| `MarkdownQuote` | dark gray foreground | Markdown quote content. |
| `MarkdownMarker` | dark gray foreground | Markdown heading, quote, and list markers. |
| `MessageAttachment` | cyan foreground | Attachment summaries. |
| `InlineCode` | `#FFA500` foreground | Inline code. |
| `MessageLink` | cyan foreground, underline | Raw URLs and Markdown links. |
| `MentionSelf` | yellow on `#5C4C23` | Mentions that notify the current user. Colored role mentions keep their Discord foreground. |
| `MentionOther` | `#C1CEF7` on `#28325C` | Other user, channel, and uncolored role mentions. |
| `MentionRole` | Discord role foreground and derived background | Colored role mentions that do not notify the current user. An explicit background overrides the derived background. |
| `MentionPickerRole` | magenta foreground | Role and `@everyone` picker fallback when Discord supplies no role color. |
| `EmbedGutter` | red foreground | Embed gutter fallback when Discord supplies no embed color. |
| `EmbedLink` | blue foreground, underline | Links in embeds. |
| `CommandName` | `MessageSecondary`, `#5865F2` foreground | Slash command names in system messages. |
| `SystemThreadName` | cyan foreground, bold | Actionable thread name in system messages. |
| `PollAnswerSelected` | terminal foreground, bold | Current user's selected poll answer. |
| `PollWinner` | terminal foreground, bold | Winning poll result. |
| `UnreadBanner` | terminal foreground on `#5865F2` | Unread-message banner. |
| `UnreadDivider` | `#ED4245` foreground | Unread divider line and label. |
| `ForumPinnedBadge` | yellow foreground, bold | Pinned forum-post badge. |
| `Error` | red foreground | Failures, destructive actions, and required indicators. |
| `Warning` | yellow foreground | Warnings and pending or degraded states. |
| `Success` | green foreground | Successful operations. |
| `Info` | cyan foreground | Informational feedback. |

The complete default configuration, including all linked component groups, is
in the collapsible **Default theme config** section of the project README.

## External colors

Nonzero Discord role and server-folder colors override related Highlight Group
foregrounds. Group modifiers and configured backgrounds still apply.
`MentionRole.background` replaces the background derived from a role color for
non-notifying role mentions. Notifying role mentions use `MentionSelf` with the
Discord role foreground.

Discord embed colors, including black, override `EmbedGutter.foreground`.
`EmbedGutter` is the fallback when an embed has no color. Syntect controls
fenced-code syntax colors and Concord selects its light or dark palette from
`Normal.background`. Selected rows keep Discord and presence foregrounds.
