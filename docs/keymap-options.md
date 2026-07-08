# Keymap options

> ⚠️ Keymap action names and default bindings may have breaking changes between releases.

Concord reads key settings from the `[keymap]` section in `keymap.toml`.

Example `keymap.toml`:

```toml
[keymap]
StartComposer = { keys = ["c"] }
ClosePopup = "q"
ReplyMessage = "<leader>mr"
VoiceDeafen = "<leader>vd"
VoiceMute = "<leader>vm"
VoiceLeave = "<leader>vl"

[keymap.groups]
"<leader>v" = "Voice"

[keymap.guild_actions]
LeaveServer = { keys = ["l"], description = "leave server" }

[keymap.channel_actions]
MuteChannel = { keys = ["x"], description = "mute channel" }

[keymap.message_actions]
OpenThread = { keys = ["t"], description = "open thread" }
GoToReferencedMessage = "g"

[keymap.composer]
OpenEditor = "<C-o>"
DeletePreviousWord = "<A-backspace>"
```

There are several kinds of keymap settings:

| Config path                                                                                                                            | What it controls                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `[keymap] leader`                                                                                                                      | The key that opens the leader popup. Defaults to `Space`.                                               |
| `[keymap] <ActionName>`                                                                                                                | Directly assignable UI actions such as `StartComposer`, `ClosePopup`, `ReplyMessage`, and `OpenThread`. |
| `[keymap.groups]`                                                                                                                      | Optional titles for prefix popups, such as naming `<leader>v` as `Voice`.                               |
| `[keymap.guild_actions]`, `[keymap.channel_actions]`, `[keymap.message_actions]`, `[keymap.member_actions]`, `[keymap.thread_actions]` | Shortcuts shown inside focused-pane and thread/post action menus.                                       |
| `[keymap.composer]`                                                                                                                    | Shortcuts used while the message composer is open, such as editor and cursor commands.                  |

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

Set an action to `false` or an empty string to disable that shortcut and remove
its default binding:

```toml
[keymap]
CopyMessage = false # or ""

[keymap.message_actions]
CopyMessage = ""
```

Scoped action tables use the same string or object shape, but each `keys` entry
must be a single key chord. Multi-key sequences such as `gt` are only supported
by direct `[keymap]` actions, not by `[keymap.guild_actions]`,
`[keymap.channel_actions]`, `[keymap.message_actions]`,
`[keymap.member_actions]`, or `[keymap.thread_actions]`.

```toml
[keymap.channel_actions]
MuteChannel = { keys = ["u", "<C-u>"], description = "mute channel" }

[keymap.message_actions]
GoToReferencedMessage = { keys = ["g"], description = "go to referenced message" }
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
`Backspace`, `Delete`, `Ctrl+c`, `Ctrl+n`, or `Ctrl+p`. Invalid, reserved, or
conflicting bindings are ignored for that action. `[keymap.composer]` is separate
and can remap those editing keys. `Esc` always closes active popups, and
`Ctrl+n` and `Ctrl+p` always move row selection down and up.

## Directly assignable actions

These action names can be assigned directly under `[keymap]`. Defaults
that start with `<leader>` are shown without the leading `Space` in the leader
popup. `OpenDisplayOptions`, `OpenComposerOptions`, `OpenNotificationOptions`,
and `OpenVoiceOptions` have contextual defaults inside the Options popup, so
assign your own full sequence if you want direct keys for them.

Navigation and app actions:

| Action name             | Default config             | Action                                                                     |
| ----------------------- | -------------------------- | -------------------------------------------------------------------------- |
| `StartComposer`         | `"i"`                      | Start the message composer, or open the forum/media post composer overlay. |
| `OpenPaneFilter`        | `"/"`                      | Open the focused pane filter or search.                                    |
| `ClosePopup`            | `"q"`                      | Close the active popup.                                                    |
| `FocusGuildPane`        | `"1"`                      | Show and focus the Servers pane.                                           |
| `FocusChannelPane`      | `"2"`                      | Show and focus the Channels pane.                                          |
| `FocusMessagePane`      | `"3"`                      | Focus the Messages pane.                                                   |
| `FocusMemberPane`       | `"4"`                      | Show and focus the Members pane.                                           |
| `SelectNext`            | `"j"`                      | Move selection down in navigation lists.                                   |
| `SelectPrevious`        | `"k"`                      | Move selection up in navigation lists.                                     |
| `CycleFocusNext`        | `["tab", "l", "right"]`    | Cycle focus forward.                                                       |
| `CycleFocusPrevious`    | `["<S-tab>", "h", "left"]` | Cycle focus backward.                                                      |
| `HalfPageDown`          | `"<C-d>"`                  | Half-page down.                                                            |
| `HalfPageUp`            | `"<C-u>"`                  | Half-page up.                                                              |
| `ScrollViewportDown`    | `"J"`                      | Scroll the focused pane viewport down.                                     |
| `ScrollViewportUp`      | `"K"`                      | Scroll the focused pane viewport up.                                       |
| `JumpTop`               | `"gg"`                     | Jump to the top.                                                           |
| `JumpBottom`            | `"G"`                      | Jump to the bottom.                                                        |
| `ScrollHorizontalLeft`  | `"H"`                      | Scroll focused pane horizontally left.                                     |
| `ScrollHorizontalRight` | `"L"`                      | Scroll focused pane horizontally right.                                    |
| `ResizePaneLeft`        | `["<A-h>", "<A-left>"]`    | Shrink the focused pane width.                                             |
| `ResizePaneRight`       | `["<A-l>", "<A-right>"]`   | Grow the focused pane width.                                               |
| `Quit`                  | `"q"`                      | Quit Concord.                                                              |

Message actions:

| Action name             | Default config | Action                                           |
| ----------------------- | -------------- | ------------------------------------------------ |
| `CopyMessage`           | `"y"`          | Copy selected message content.                   |
| `ReactMessage`          | `"r"`          | Add or remove a reaction.                        |
| `ReplyMessage`          | `"R"`          | Start a reply.                                   |
| `DeleteMessage`         | `"d"`          | Open delete confirmation.                        |
| `EditMessage`           | `"e"`          | Start editing the selected message.              |
| `OpenMessageUrl`        | `"o"`          | Open the selected message URL.                   |
| `RemoveMessageEmbeds`   | none           | Remove embeds from the selected message.         |
| `PlayMedia`             | `"x"`          | Play selected video media in an external player. |
| `ViewMessageAttachment` | `"v"`          | Open the selected message attachment viewer.     |
| `GoToReferencedMessage` | none           | Go to the replied or forwarded message.          |
| `ShowMessageProfile`    | none           | Open the selected message author's profile.      |
| `PinMessage`            | none           | Open pin or unpin confirmation.                  |
| `OpenThread`            | none           | Open the selected message's thread.              |
| `ShowReactionUsers`     | none           | Show users who reacted to the selected message.  |
| `OpenPollVotePicker`    | none           | Choose poll votes for the selected message.      |

Pane, options, and voice actions:

| Action name               | Default config                     | Action                                       |
| ------------------------- | ---------------------------------- | -------------------------------------------- |
| `ToggleGuildPane`         | `"<leader>1"`                      | Toggle the Servers pane.                     |
| `ToggleChannelPane`       | `"<leader>2"`                      | Toggle the Channels pane.                    |
| `ToggleMemberPane`        | `"<leader>4"`                      | Toggle the Members pane.                     |
| `OpenFocusedPaneAction`   | `"<leader>a"`                      | Open actions for the currently focused pane. |
| `OpenCurrentUserProfile`  | `"<leader>p"`                      | Open your profile settings popup.            |
| `OpenOptions`             | `"<leader>o"`                      | Open the options category picker.            |
| `ChannelSwitcher`         | `"<leader><leader>"`               | Open channel switcher.                       |
| `OpenNotificationInbox`   | `"<leader>n"`                      | Open the notification inbox.                 |
| `OpenDisplayOptions`      | Contextual `d` after `OpenOptions` | Open Display options.                        |
| `OpenComposerOptions`     | Contextual `c` after `OpenOptions` | Open Composer options.                       |
| `OpenNotificationOptions` | Contextual `n` after `OpenOptions` | Open Notification options.                   |
| `OpenVoiceOptions`        | Contextual `v` after `OpenOptions` | Open Voice options.                          |
| `VoiceDeafen`             | `"<leader>vd"`                     | Toggle voice deafen.                         |
| `VoiceMute`               | `"<leader>vm"`                     | Toggle voice mute.                           |
| `VoiceLeave`              | `"<leader>vl"`                     | Leave the current Concord voice channel.     |

Profile popup shortcuts are fixed once `OpenCurrentUserProfile` opens your
profile settings popup:

| Shortcut | Action                              |
| -------- | ----------------------------------- |
| `o`      | sign out, delete saved credentials. |

## Composer actions

These action names can be assigned under `[keymap.composer]`. Configured keys
replace that action's defaults. Any printable single character can be configured,
but that key will run the composer action instead of inserting text.

These shortcuts also apply inside the forum/media post composer overlay for
shared actions such as paste, submit, close, clear, attachment removal, and
cursor movement while editing a field. In the overlay, `Tab` and `Shift+Tab`
move between title, body, attachments, and tags. `Enter` starts or finishes
editing title/body, removes the selected attachment while choosing attachments,
or toggles the selected tag. Paste files or images while editing the body to add
attachments. Press `s` outside edit mode to create the post.

| Composer action        | Default config                                     | Action                                  |
| ---------------------- | -------------------------------------------------- | --------------------------------------- |
| `OpenEditor`           | `"<C-e>"`                                          | Open the current draft in `$EDITOR`.    |
| `PasteClipboard`       | `"<C-v>"`                                          | Request clipboard paste.                |
| `InsertNewline`        | `["<C-j>", "<S-enter>", "<C-enter>", "<A-enter>"]` | Insert a newline.                       |
| `Submit`               | `"enter"`                                          | Submit the composer.                    |
| `Close`                | `"esc"`                                            | Close the composer.                     |
| `ClearInput`           | `"<C-c>"`                                          | Clear the composer input.               |
| `RemoveLastAttachment` | `"delete"`                                         | Remove the last pending attachment.     |
| `DeletePreviousChar`   | `"backspace"`                                      | Delete the previous character.          |
| `DeletePreviousWord`   | `["<A-backspace>", "<C-backspace>", "<C-w>"]`      | Delete the word before the cursor.      |
| `DeleteToLineStart`    | `"<C-u>"`                                          | Delete to the start of the current line. |
| `DeleteToLineEnd`      | `"<C-k>"`                                          | Delete to the end of the current line.   |
| `MoveCursorUp`         | `"up"`                                             | Move the cursor up.                     |
| `MoveCursorDown`       | `"down"`                                           | Move the cursor down.                   |
| `MoveCursorWordLeft`   | `"<C-left>"`                                       | Move the cursor one word left.          |
| `MoveCursorLeft`       | `"left"`                                           | Move the cursor left.                   |
| `MoveCursorWordRight`  | `"<C-right>"`                                      | Move the cursor one word right.         |
| `MoveCursorRight`      | `"right"`                                          | Move the cursor right.                  |
| `MoveCursorHome`       | `"home"`                                           | Move the cursor to the start.           |
| `MoveCursorEnd`        | `"end"`                                            | Move the cursor to the end.             |
| `ToggleReplyPing`      | `"<A-p>"`                                          | Toggle whether replies ping the author. |

## Focused pane actions

`OpenFocusedPaneAction` opens the action menu for the pane that currently has
focus. Server, channel, message, and member pane actions can be configured in
scoped tables. Focused-pane action menus keep their actions visible, and actions
that do not apply to the current selection are shown dimmed and disabled.

Server pane actions:

```toml
[keymap.guild_actions]
MarkAsRead = { keys = ["m"], description = "mark server as read" }
MuteServer = { keys = ["u"], description = "mute server" }
LeaveServer = { keys = ["l"], description = "leave server" }
FolderSettings = { keys = ["r"], description = "folder settings" }
```

| Scoped action    | Default | Action                                                                     |
| ---------------- | ------- | -------------------------------------------------------------------------- |
| `MarkAsRead`     | `m`     | Mark all unread viewable channels in the selected server read.             |
| `MuteServer`     | `u`     | Mute or unmute the selected server. Also accepts `ToggleMute` as an alias. |
| `LeaveServer`    | `l`     | Open confirmation to leave the selected server.                            |
| `FolderSettings` | `r`     | Edit the selected server folder name and color.                            |

Channel pane actions:

```toml
[keymap.channel_actions]
JoinVoice = { keys = ["e"], description = "join voice" }
LeaveVoice = { keys = ["l"], description = "leave voice" }
ShowPinnedMessages = { keys = ["p"], description = "show pinned messages" }
ShowThreads = { keys = ["t"], description = "show threads" }
MarkAsRead = { keys = ["m"], description = "mark as read" }
MuteChannel = { keys = ["u"], description = "mute channel" }
```

| Scoped action        | Default | Action                                                                                      |
| -------------------- | ------- | ------------------------------------------------------------------------------------------- |
| `JoinVoice`          | `e`     | Join the selected voice channel.                                                            |
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

Message pane actions:

```toml
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
```

| Scoped action           | Default | Action                                           |
| ----------------------- | ------- | ------------------------------------------------ |
| `CopyMessage`           | `y`     | Copy selected message content.                   |
| `ReactMessage`          | `r`     | Add or remove a reaction.                        |
| `ReplyMessage`          | `R`     | Start a reply.                                   |
| `DeleteMessage`         | `d`     | Open delete confirmation.                        |
| `EditMessage`           | `e`     | Start editing the selected message.              |
| `OpenMessageUrl`        | `o`     | Open the selected message URL.                   |
| `RemoveMessageEmbeds`   | `D`     | Remove embeds from the selected message.         |
| `PlayMedia`             | `x`     | Play selected video media in an external player. |
| `ViewMessageAttachment` | `v`     | Open the selected message attachment viewer.     |
| `GoToReferencedMessage` | `g`     | Go to the replied or forwarded message.          |
| `ShowMessageProfile`    | `p`     | Open the selected message author's profile.      |
| `PinMessage`            | `P`     | Open pin or unpin confirmation.                  |
| `OpenThread`            | `t`     | Open the selected message's thread.              |
| `ShowReactionUsers`     | `u`     | Show users who reacted to the selected message.  |
| `OpenPollVotePicker`    | `c`     | Choose poll votes for the selected message.      |

Direct `[keymap]` message action bindings are separate. For example,
`GoToReferencedMessage = "gd"` under `[keymap]` makes `gd` work directly from the
Messages pane, while `[keymap.message_actions] GoToReferencedMessage = "g"`
controls the action menu shortcut.

Thread and post actions:

The thread action menu opens for a focused thread in the Channels pane or a
forum post selected in the thread/post list. `Pin` only appears for forum posts.

```toml
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
```

| Scoped action          | Default | Action                                                         |
| ---------------------- | ------- | -------------------------------------------------------------- |
| `MarkAsRead`           | `m`     | Mark the thread or post read.                                  |
| `ToggleFollow`         | `f`     | Follow or unfollow the thread or post.                         |
| `Close`                | `c`     | Close or reopen the thread or post. Needs author or moderator. |
| `Lock`                 | `l`     | Lock or unlock the thread or post. Moderator only.             |
| `Edit`                 | `e`     | Edit title, tags, slow mode, and auto-archive.                 |
| `CopyLink`             | `y`     | Copy a link to the thread or post.                             |
| `ToggleMute`           | `u`     | Mute or unmute the thread or post. Needs following first.      |
| `NotificationSettings` | `n`     | Choose the notification level for the thread or post.          |
| `Pin`                  | `P`     | Pin or unpin the post. Forum posts only, moderator only.       |
| `Delete`               | `d`     | Delete the whole thread or post. Moderator only.               |
| `CopyId`               | `i`     | Copy the thread or post ID.                                    |

Scoped action `description` changes the label shown in server, channel,
message, member, and thread action menus. Multiple configured `keys` work as aliases
when they are unique in the current action menu, and the popup shows them
together, such as `[x/u]`. If two actions in the same menu use the same
configured key, that key is ignored for both actions. If
an action has no unique configured key, it falls back to `1` through `9`, then
`0`.
