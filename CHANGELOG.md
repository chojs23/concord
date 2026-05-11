# Changelog

All notable changes to this project will be documented in this file.

## [1.3.1] - 2026-05-11

### Bug Fixes

- Fix test cache leakage (#31) in [#31](https://github.com/chojs23/concord/pull/31) by @chojs23
- Discord Gateway startup failure (#36) in [#36](https://github.com/chojs23/concord/pull/36) by @chojs23
- Fix muted channels contributing to server unread badges (#37) in [#37](https://github.com/chojs23/concord/pull/37) by @chojs23

### Documentation

- Update readme by @chojs23

### Features

- Add server mark-as-read & active channel ack debounce by @chojs23
- Display online and member count (#16) in [#16](https://github.com/chojs23/concord/pull/16) by @lisk77
- Prefs persistence (#25) in [#25](https://github.com/chojs23/concord/pull/25) by @TobyBridle
- Support muting channels and servers via actions (#26) in [#26](https://github.com/chojs23/concord/pull/26) by @nexxai
- Support all emojis for reactions (#30) in [#30](https://github.com/chojs23/concord/pull/30) by @nexxai
- Add emoji picker and autocomplete in composer (#34) in [#34](https://github.com/chojs23/concord/pull/34) by @chojs23

### Miscellaneous Tasks

- Remove cache warm by @chojs23

### Refactor

- Refactor quick reaction emoji labels to use emojis crate (#32) in [#32](https://github.com/chojs23/concord/pull/32) by @chojs23
- Remove dead test by @chojs23

## [1.3.0] - 2026-05-10

### Bug Fixes

- Prevent scrolling past last message (#12) in [#12](https://github.com/chojs23/concord/pull/12) by @lisk77
- Display of self mutuals (#14) in [#14](https://github.com/chojs23/concord/pull/14) by @lisk77

### Documentation

- Update README.md by @chojs23

### Features

- Support resizing panes (#18) in [#18](https://github.com/chojs23/concord/pull/18) by @nexxai
- *(composer)* Add CTRL+Left, CTRL+Right word skip (#15) in [#15](https://github.com/chojs23/concord/pull/15) by @TobyBridle
- Add search to reaction picker + allow toggling of reactions (#19) in [#19](https://github.com/chojs23/concord/pull/19) by @nexxai
- Add fuzzy score helper (#21) in [#21](https://github.com/chojs23/concord/pull/21) by @chojs23
- Typo tolerant fuzzy (#22) in [#22](https://github.com/chojs23/concord/pull/22) by @chojs23

### Refactor

- *(composer)* Use char-safe word boundary helpers (#20) in [#20](https://github.com/chojs23/concord/pull/20) by @chojs23
- Store app files under $XDG_CONFIG_HOME/concord (#23) in [#23](https://github.com/chojs23/concord/pull/23) by @chojs23


### New Contributors

- @lisk77 made their first contribution in [#14](https://github.com/chojs23/concord/pull/14)
- @nexxai made their first contribution in [#19](https://github.com/chojs23/concord/pull/19)
- @TobyBridle made their first contribution in [#15](https://github.com/chojs23/concord/pull/15)
## [1.2.0] - 2026-05-09

### Documentation

- Update readme for upload attachment by @chojs23
- Remove some composer key usage by @chojs23

### Features

- Add leader by @chojs23
- Search channel with fuzzy by @chojs23
- Add new version available notice at header by @chojs23

### Miscellaneous Tasks

- Allign Release build toolchain with CI chace warmer by @chojs23

### Refactor

- Move options key to leader by @chojs23

## [1.1.2] - 2026-05-09

### Bug Fixes

- Redraw visible unread sidebar updates during image rendering by @chojs23
- Active-channel auto-ack by @chojs23

### Features

- Multipart file upload (#8) in [#8](https://github.com/chojs23/concord/pull/8) by @chojs23

### Miscellaneous Tasks

- Tighten dist profile with full LTO, single codegen unit, and strip by @chojs23

### Tests

- Consolidate redundant split tests by @chojs23

### Cd

- Protect release by @chojs23
- Improve build caching by @chojs23

## [1.1.1] - 2026-05-09

### Bug Fixes

- Keep original image width:height ratio (#6) in [#6](https://github.com/chojs23/concord/pull/6) by @chojs23

### Cd

- Chain crates publish after release by @chojs23

## [1.1.0] - 2026-05-09

### Bug Fixes

- *(flake)* Use crane (#4) in [#4](https://github.com/chojs23/concord/pull/4) by @M4jor-Tom

### Documentation

- Add CONTRIBUTING.md by @chojs23
- Update readme by @chojs23
- Update readme by @chojs23

### Features

- Add terminal notification (#5) in [#5](https://github.com/chojs23/concord/pull/5) by @chojs23

### Miscellaneous Tasks

- Enable generated github release notes by @chojs23


### New Contributors

- @M4jor-Tom made their first contribution in [#4](https://github.com/chojs23/concord/pull/4)
## [1.0.3] - 2026-05-08

### Bug Fixes

- Install rustls ring provider by @chojs23
- Surface user-account READY via raw JSON fallback and silence twilight deserialize errors by @chojs23
- Expand user-account fallback parser to cover guild, channel, message, and member events by @chojs23
- Make server folder toggles reliable by @chojs23
- Send raw user token for REST requests by @chojs23
- Start dashboard without message selection by @chojs23
- Wait for channel selection before showing messages by @chojs23
- Keep pane scrolling state-owned by @chojs23
- Align scroll height with pane content by @chojs23
- Keep startup channel pane inactive by @chojs23
- Preserve channel cursor across DM sorting by @chojs23
- Retry failed message history loads by @chojs23
- Require selected channel before composing by @chojs23
- Use text color for active selections by @chojs23
- Enable modified key reporting by @chojs23
- Keep proxy-only attachment URLs by @chojs23
- Disable attachment open shortcut by @chojs23
- Render image messages inline by @chojs23
- Render previews while scrolling by @chojs23
- Preview all visible images by @chojs23
- Keep image previews anchored by @chojs23
- Keep preview scrolling row-aware by @chojs23
- Dispatch image preview requests by @chojs23
- Scale image preview height by @chojs23
- Reduce wide preview spacing by @chojs23
- Avoid upscaling small previews by @chojs23
- Keep preview items within scroll bounds by @chojs23
- Preserve image previews in scrolloff by @chojs23
- Render clipped image previews by @chojs23
- Classify video attachments separately by @chojs23
- Skip videos in image preview layout by @chojs23
- Label video attachments in messages by @chojs23
- Handle QR captcha without browser fallback by @chojs23
- Align forwarded image preview targets by @chojs23
- Dim forwarded metadata line by @chojs23
- Separate reply preview author by @chojs23
- Label unsupported message types by @chojs23
- Filter message actions by subtype by @chojs23
- Remove poll results action by @chojs23
- Remove unavailable poll vote action by @chojs23
- Preserve newest messages during pagination by @chojs23
- Hide overflowing image previews by @chojs23
- Wrap message content lines by @chojs23
- Use display width for message wrapping by @chojs23
- Scroll messages by rendered line by @chojs23
- Clamp message scroll by rendered rows by @chojs23
- Separate message selection and viewport scroll by @chojs23
- Avoid message selection overflow by @chojs23
- Text clipping by @chojs23
- Use guild aliases for message authors by @chojs23
- Clip message rows by display width by @chojs23
- Remove some unnecessary env by @chojs23
- Use concord logging settings by @chojs23
- Use concord credential path by @chojs23
- Bottom align initial message viewport by @chojs23
- Hide avatar placeholder on message content rows by @chojs23
- Keep message highlight out of avatar column by @chojs23
- Remove bold from selection highlight by @chojs23
- Show message times in local timezone by @chojs23
- Align preview rows with rendered mentions by @chojs23
- Render mentions from message metadata by @chojs23
- Preserve mention metadata across merges by @chojs23
- Prefer guild aliases for mentions by @chojs23
- Normalize mention highlight ranges by @chojs23
- Add default app constructor by @chojs23
- Simplify mention highlight filtering by @chojs23
- Clear stale message update metadata by @chojs23
- Avoid download filename races by @chojs23
- Send real Discord replies by @chojs23
- Load custom guild emojis for reactions by @chojs23
- Bound image preview cache memory by @chojs23
- Move image preview decoding off tui loop by @chojs23
- Render user join messages by @chojs23
- Show known system message labels by @chojs23
- Cache Discord thread gateway channels by @chojs23
- Cache Discord member chunks by @chojs23
- Request members for active guilds by @chojs23
- Group members by Discord roles by @chojs23
- Truncate member names by display width by @chojs23
- Preserve DM recipient presence state by @chojs23
- Show DM recipients as members by @chojs23
- Color DM channels by recipient status by @chojs23
- Parse DM recipient presence updates by @chojs23
- Subscribe to active DM presence by @chojs23
- Hydrate active guild member lists by @chojs23
- Include self in group DM recipients by @chojs23
- Update poll vote state by @chojs23
- Submit poll votes together by @chojs23
- Process all member list sync ranges by @chojs23
- Receive unknown gateway events for member list updates by @chojs23
- Open member profile on enter or space by @chojs23
- Show loaded versus total guild members without full member loading by @chojs23
- Scope user profile cache by guild by @chojs23
- Show user profile load failures by @chojs23
- Update member totals on removal events by @chojs23
- Count fallback poll result votes by @chojs23
- Account for separators in image preview targets by @chojs23
- Hide channels that doesn't meet permission by @chojs23
- Guild user role by @chojs23
- Fix startup presense fallback by @chojs23
- Fix overflowing scrollbar by @chojs23
- Fix some ui by @chojs23
- Fall back when keyboard progressive enhancement unsupported by @chojs23
- Image blinking by @chojs23
- Message box by @chojs23
- Fix message viewport junmping with empty space by @chojs23
- Fix message viewport auto-follow by @chojs23
- Remove new dm by @chojs23
- Fix image flicker by suppressing redundant effect-driven redraws by @chojs23
- Clamp horizontal pane scrolling by @chojs23
- Remove inactive message scroll hint by @chojs23
- Preserve active guild mention style by @chojs23
- Prevent member activity row from overflowing panel by @chojs23
- Handle zero Discord snowflake parsing in gateway read state. by @chojs23
- Make qr code more scannable and fix partial paste issue by @chojs23
- Fix self-authored Discord messages being marked as unread by @chojs23

### Documentation

- Add curl install instructions by @chojs23
- Clarify release installer path by @chojs23
- Add example image by @chojs23
- Update readme by @chojs23
- Update example by @chojs23

### Features

- Add configuration and error foundation by @chojs23
- Add Discord gateway and messaging client by @chojs23
- Wire the app runtime to the Discord client by @chojs23
- Add credential-backed token config by @chojs23
- Add terminal dashboard and login UI by @chojs23
- Wire saved-token login into startup by @chojs23
- Store credentials in local file by @chojs23
- Add Discord state model by @chojs23
- Render Discord state panes by @chojs23
- Wire message composer commands by @chojs23
- Add QR code login flow with mode-select login screen by @chojs23
- Rebuild dashboard as Discord-style 4-pane layout with members and presence by @chojs23
- Extract guilds and DM channels from user-account READY and add gateway debug log by @chojs23
- Surface direct messages in the guild pane with @ channel prefix by @chojs23
- Group servers by Discord folders with collapsible headers by @chojs23
- Improve server folder tree rendering by @chojs23
- Store channel tree metadata by @chojs23
- Render collapsible channel tree by @chojs23
- Activate servers and channels on confirm by @chojs23
- Sort direct messages by latest message by @chojs23
- Add recent message history loading by @chojs23
- Add global file logging by @chojs23
- Sort direct messages by recent activity by @chojs23
- Add lazyagent-style message scrolling by @chojs23
- Use consistent scroll behavior across panes by @chojs23
- Clarify server and channel selection by @chojs23
- Mark active server in guild list by @chojs23
- Use four-pane dashboard layout by @chojs23
- Support multiline message input by @chojs23
- Preserve message attachment metadata by @chojs23
- Render message attachments in pane by @chojs23
- Open message attachments externally by @chojs23
- Load attachment preview data by @chojs23
- Render inline image previews by @chojs23
- Preserve forwarded message snapshots by @chojs23
- Preview forwarded attachments by @chojs23
- Render forwarded messages by @chojs23
- Add password login with MFA by @chojs23
- Preserve forwarded source metadata by @chojs23
- Render forwarded message cards by @chojs23
- Separate attachment summary lines by @chojs23
- Render Discord message types by @chojs23
- Render reply previews by @chojs23
- Render poll messages by @chojs23
- Add message actions for polls by @chojs23
- Paginate older message history by @chojs23
- Add partial clipping for inline image by @chojs23
- Render Discord-like message rows by @chojs23
- Render message avatars by @chojs23
- Render user mentions in messages by @chojs23
- Highlight current user mentions by @chojs23
- Add emoji reaction picker by @chojs23
- Support custom guild emoji reactions by @chojs23
- Show custom emoji images in picker by @chojs23
- Show reply target in composer by @chojs23
- Render Discord system message cards by @chojs23
- Queue guild member gateway requests by @chojs23
- Add guild member load command by @chojs23
- Cache Discord role metadata by @chojs23
- Store DM channel recipients by @chojs23
- Render DM recipients in member pane by @chojs23
- Add reaction pin and poll actions by @chojs23
- Render polls in boxed cards by @chojs23
- Render custom emoji on message by @chojs23
- List threads via channel actions menu by @chojs23
- Subscribe member list on guild switch by @chojs23
- Scroll member list subscription with viewport by @chojs23
- Split member sidebar into role online and offline groups by @chojs23
- Load and cache discord user profiles by @chojs23
- Capture friend relationships from ready by @chojs23
- Open user profile popup from message action menu by @chojs23
- Open user profile from member action menu by @chojs23
- Render profile avatar image inside the popup by @chojs23
- React to relationship add and remove dispatches by @chojs23
- Jump to mutual server from profile popup by @chojs23
- Change message author names to bold white and profile popup names to status-based color by @chojs23
- Add debug panel and youtube embed by @chojs23
- Gate sidebar with Discord's view-channel permission by @chojs23
- Add test log path by @chojs23
- Add scrollbar by @chojs23
- Thread box by @chojs23
- Improve forum by @chojs23
- Pane actions by @chojs23
- Add gateway reaction event handling by @chojs23
- Cd by @chojs23
- Mouse by @chojs23
- Handle large traffic event by @chojs23
- Migrated to use watch revision signal plus sequenced effect stream hybrid by @chojs23
- Selection with mouse by @chojs23
- Use iterm2 protocol for wezterm by @chojs23
- Fix some mouse interaction and optimize image redraw by @chojs23
- Render message inline emoji by @chojs23
- Multiple image viewer by @chojs23
- WIP message selection box by @chojs23
- Split forum posts by @chojs23
- Auto scroll by @chojs23
- Keep input mode by @chojs23
- Message update & delete by @chojs23
- New messages by @chojs23
- Download non-image attachments from message menu by @chojs23
- Render sticker-only messages instead of empty body by @chojs23
- Send DM from profile popup with m shortcut by @chojs23
- Render role mentions as @role-name with highlight by @chojs23
- Scroll-based profile popup with mouse wheel support by @chojs23
- Add client headers and cookie by @chojs23
- Channel tag by @chojs23
- Embed image resize + move emojj under image by @chojs23
- Unread/read channel by @chojs23
- Unread messages by @chojs23
- Add header by @chojs23
- Add option by @chojs23
- Add horizontal pane scrolling by @chojs23
- Highlight self reactions in yellow by @chojs23
- Show startup loading state by @chojs23
- Report startup token problems by @chojs23
- Show guild mention badges by @chojs23
- Add unread count for dm by @chojs23
- Action shortcuts by @chojs23
- User activity by @chojs23
- Add configurable image quality presets by @chojs23
- Nix flake by @chojs23

### Miscellaneous Tasks

- Initialize Rust application scaffold by @chojs23
- Clarify message input wording by @chojs23
- Show temporary message type prefix by @chojs23
- Rename package to concord by @chojs23
- Remove twilight by @chojs23
- Split some states by @chojs23
- Bump dependencies to latest by @chojs23
- Ui split by @chojs23
- Split tui modules by @chojs23
- Split discord gateway and state by @chojs23
- Fix some issues by @chojs23
- Remove clippy validation by @chojs23
- Split parser and redraw modules by @chojs23

### Performance

- Throttle inline image previews by @chojs23
- Bound attachment preview downloads by @chojs23
- Reduce retained media cache memory by @chojs23
- Bound older history retention by @chojs23
- Lower cache limits by @chojs23
- Resize discord cdn images by @chojs23
- Youtube embed thumbnail resizing by @chojs23

### Refactor

- Remove unused Discord state fields by @chojs23
- Sync viewport heights before rendering by @chojs23
- Compose message preview rows by @chojs23
- Use message capabilities for actions by @chojs23
- Construct app without config by @chojs23
- Remove startup config module by @chojs23
- Group message update state fields by @chojs23
- Remove dead attachment helpers by @chojs23
- Reuse message viewport bottom alignment by @chojs23
- Extract tui message formatting by @chojs23
- Split tui media target discovery by @chojs23
- Extract message viewport row scrolling by @chojs23
- Derive message height from formatter by @chojs23
- Unify message content span styling by @chojs23
- Extract image preview media cache by @chojs23
- Share emoji picker visible limit by @chojs23
- Split poll state by @chojs23
- Split message action state by @chojs23
- Split reaction state by @chojs23
- Split composer state by @chojs23

### Revert

- Remove latest-message direct message sorting by @chojs23

### Style

- Format mention highlight mapping by @chojs23
- Dim reply target composer hint by @chojs23
- Color role-grouped members by status by @chojs23

### Tests

- Cover raw ready custom emojis by @chojs23
- Cover cached thread channel deletion by @chojs23
- Replace Korean UI test fixtures by @chojs23
- Replace Korean TUI helper fixtures by @chojs23
- Simplify tui test helpers by @chojs23
- Reuse ui message content fixture by @chojs23

### Build

- Add TUI and keychain dependencies by @chojs23


### New Contributors

- @chojs23 made their first contribution
- @vncsalencar made their first contribution

