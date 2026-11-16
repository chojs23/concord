# Voice-state export protocol

Concord exposes a read-only local endpoint for voice overlays. It does not
implement Discord's voice RPC commands and does not draw the overlay itself.

## Enable or disable export

Export is enabled by default. Change **Options → Voice → Export voice state**
to apply the setting immediately and save it to `config.toml`:

```toml
[voice]
export_state = true
```

Disabling export closes the listener and all connected clients. Enabling it
starts a new listener. Export belongs to the signed-in Concord session and
stops on logout or exit. Bind failures are logged and retried every five seconds.

## Endpoints and access

| Platform | Endpoint |
| --- | --- |
| Unix with `XDG_RUNTIME_DIR` | `$XDG_RUNTIME_DIR/concord-voice.sock` |
| Unix fallback | `$XDG_STATE_HOME/concord/voice/concord-voice.sock`, or `~/.local/state/concord/voice/concord-voice.sock` |
| Windows | `\\.\pipe\concord-voice` |

An explicitly set `XDG_RUNTIME_DIR` must be an absolute, private directory owned
by the current user. Invalid directories do not trigger a fallback. The Unix
socket has mode `0600`, and the fallback `voice` directory has mode `0700`.
Concord removes only its own socket on close. A private lockfile remains so
multiple Concord processes cannot race to replace the endpoint.

The Windows named pipe accepts local connections only and grants access to the
current user's SID. Another live listener is not replaced. If several Concord
sessions run at once, only one owns this fixed endpoint.

Local programs running as the same user can read channel names, participant
identities, avatar URLs, and voice flags. Audio, messages, credentials, session
IDs, and tokens are not exported. Turn export off if this sharing is unwanted.

## Wire format

The server sends UTF-8 JSON, one complete object followed by a newline per
snapshot. Clients must not send data or a handshake. The first snapshot arrives
on connection. Later snapshots replace the complete previous state.

This example is formatted for reading. On the wire it occupies one line:

```json
{
  "v": 1,
  "self_id": "123",
  "channel": {"id": "456", "name": "General", "guild_id": "789"},
  "members": [
    {
      "id": "123",
      "name": "Alice",
      "avatar": null,
      "speaking": false,
      "mute": false,
      "deaf": false,
      "self_mute": false,
      "self_deaf": false,
      "stream": false
    }
  ]
}
```

- `v` is the protocol version. It changes only for breaking changes.
- IDs are strings because Discord snowflakes can exceed JavaScript's safe integer range.
- `self_id` is null before the current user is known.
- `channel` is null outside a voice channel, with `members` set to `[]`.
- `channel.guild_id` is null for DM and group-DM calls.
- Members include the current user and are ordered by numeric user ID.
- `avatar` is a cached URL or null. Clients choose their own default image.
- `mute` and `deaf` are server flags. `self_mute` and `self_deaf` are user flags.
- `stream` means the member is sharing a screen.
- Clients must ignore unknown fields.

## Updates and reconnects

Identical snapshots are not resent. Rapid updates are coalesced to the latest
state rather than queued as an event history. A client may skip brief speaking
edges, so it must use each full snapshot as its current state.

At most 16 clients are served at once. A write stalled for two seconds closes
that client without blocking other clients or Concord's voice event processing.
Clients should reconnect with a delay after EOF or an error and clear their
display while disconnected. Concord can close connections when export is
disabled, the session ends, or the endpoint needs to restart.
