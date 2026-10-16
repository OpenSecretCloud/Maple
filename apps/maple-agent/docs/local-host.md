# Local host and ACP

Desktop and `maple-agent acp` attach to one on-demand process, started by the
same installed executable as `maple-agent host serve`. The host owns credentials,
account configuration, task files, the task index, Pi runs, tools and child
processes. Desktop owns presentation and native permission prompts. ACP projects
host operations and events onto the client protocol. Pi itself is unchanged.

## Lifetime and isolation

Opening an ACP editor can start the host without opening a window. Desktop joins
it later. Closing a client detaches; a run continues while another client remains.
After the last intentional desktop Quit or ACP detach, a two-second grace period
allows a replacement to attach, then the host cancels and drains its work and exits.
An unexpected desktop connection loss instead reserves thirty seconds for recovery,
including when another client briefly connects and leaves during that interval.
Attached clients keep the host alive. `maple-agent host stop`
explicitly stops this profile and all its tasks, even with clients attached.
There is no login item, permanent service, remote listener or remote keepalive.

One OS user, build channel and canonical config/data roots identify the storage
owner. Startup and lifetime file locks prevent two owners. Backend URL, billing
URL, client identity, attestation environment, executable source version and
compiled features must match before a client joins. A mismatched client reports
an error; it does not terminate or replace the owner. Desktop shows a startup
error window instead of panicking. Explicit `maple-agent host stop` can stop the
owner of the same storage namespace even when its build or backend settings
differ; this exception does not allow other commands. Hosts predating that stop
exception must be stopped using their matching build/configuration. Quit the old
build before switching versions. A replacement waits for a shutting-down host to
release its ownership lock before opening shared state. Rebuilds of uncommitted
source share a source version, so stop
the development host before rebuilding.

Unix uses an owner-only socket inside a private directory and checks peer UID.
Windows uses a local named pipe restricted to its owner and SYSTEM and verifies
the pipe owner. This is an OS-user trust boundary, not a sandbox against other
programs running as that user. No API token is placed in endpoint names or logs.
Signing out revokes existing runtime handles and ACP connections before another
account can sign in. Task commands validate their account at the host boundary.
Checking saved credentials and attaching ACP share a read lock with local operations;
slow credential validation does not block history, settings, or Stop. Actual account
switches remain exclusive so a request cannot cross accounts while it is admitted.

## Shared tasks and recovery

A new draft remains absent from task lists until the first accepted message.
Desktop and ACP can load the same task and observe its history and live stream.
Each task has one running loop. Desktop retains its queue/steer controls; ACP uses
the steering extension below. Cancel from either interface stops the shared run.
Filesystem paths and tools always refer to the host machine.

Snapshots include live partial rows with an event cutoff. Clients discard older
updates and keep later ones, rather than appending the same partial text twice.
The event stream is bounded. A desktop that falls behind or loses its socket
reconnects for up to fifteen seconds, without starting a replacement host. It shows
the connection state and blocks new sends until task history, active runs, questions,
and queues have been refreshed. Unsent composer text stays in place. A snapshot
also resets host-local revisions, so a restarted host cannot leave old partials
on screen. An account change discovered during recovery switches the screen before
new events are shown. Failed recovery asks the user to reopen Maple.

Commands are never replayed on reconnection. A host crash does not restart inference.
Stored history survives; active tools are not promised to survive a host crash.
An unrecovered last-client disconnect eventually drains work after the recovery
grace. This bounded local policy is not remote-mode keepalive; a future enabled
remote host must have a lifetime independent of its currently attached clients.

Desktop sends carry unique IDs. A small private `send-receipts.db` records admission
and its result, so a lost reply can be queried without resending the prompt. A
crash during admission produces an explicit unknown outcome: inspect the task
before retrying. Receipts contain hashes and task/run IDs, not prompt text or
images. They are not a token event log. IPC frames are bounded at 160 MiB to fit
the existing attachment allowance; oversized requests fail explicitly.

## ACP compatibility

ACP requires an existing sign-in. With no saved account, the command prints a
sign-in instruction to stderr and exits unsuccessfully; stdout remains protocol-only.

The adapter uses stable ACP protocol version 1. It supports initialization,
new/load/list/delete, prompts, cancellation, model and `thought_level` config
options, command discovery, images, tool/thought/text updates, usage, and negotiated
form elicitation. It does not advertise session modes or `session/close`: ACP close
means cancellation, while this host's pipe EOF means detach. A client may still
explicitly cancel before closing; Maple honors that cancellation.

Stable ACP has no steer or queue method. Maple advertises
`_meta.steering.supported=true` and implements `_session/steering`, shared with the
Codex and Claude adapters. It accepts the same `sessionId` and `prompt` content as
`session/prompt` and injects at Pi's next steering opportunity. The result is
`{"outcome":"injected"}`. With `_meta.steering.idleBehavior="promptRequired"`, an
idle task returns `{"outcome":"promptRequired","reason":"noRunningTurn"}` without
consuming the message. Legacy callers omitting that option start a new turn and
receive `{"outcome":"startedNewTurn"}`. Ordinary overlapping `session/prompt`
requests return busy; clients should use the advertised extension.

Questions appear on capable attached interfaces; the first answer resolves the
host request. Without a capable responder, the tool asks for a normal message
instead of waiting indefinitely. Client filesystem and terminal callbacks are
not used. Host-configured MCP servers stay host-owned. ACP-supplied bindings are
transient, disabled on detach and require explicit reattachment. Conflicting
server names are rejected rather than silently replacing another client's tools.

### Paseo

Configure a provider extending `acp` with command `["/path/to/maple-agent", "acp"]`.
It must inherit the same profile environment as desktop. In a managed development
workspace use its launcher (or source its generated Agent environment before
executing the built binary). A different profile is intentionally a different host.

The source baseline inspected for this change is Paseo `b2ca7dcf` (2026-10-15).
Its generic ACP provider supports the normal create/resume, prompt, tool streaming,
model/thought-level and cancellation path. It does not call `_session/steering`
and rejects an active second turn itself. Live validation with Paseo 0.11.1 also
confirmed desktop-originated user and assistant messages reach an already attached,
idle Paseo client, which can then continue the task without reopening it. Stable
ACP v1 does not supply a separate running/idle lifecycle for turns started by
another client, so transcript delivery does not establish full desktop status parity.
ACP cannot retract already rendered history on compaction;
reload is the authoritative view after history replacement. No Paseo-specific
wire shims or separate task database are added.

## Maintenance boundary

Keep host commands private and versioned; ACP remains a standards-based projection.
Test ownership, admission, snapshot/stream reconciliation, account revocation and
resource cleanup where they cross boundaries. Native macOS permissions, Windows
named pipes and Linux sockets each require platform execution evidence. Remote
pairing and transport belong to a later phase, not this local IPC implementation.
