# Desktop remote access

Remote access is off by default. The local `maple-agent host serve` daemon owns
its device identity, pairing records and Iroh endpoint. Desktop is a local IPC
client; selecting another computer opens a second, host-scoped IPC connection
through that daemon. The selected computer owns the tasks, workspace, tools,
MCP servers, computer use and send receipts. Its desktop and local ACP clients
continue to share those tasks. ACP is not exposed over Iroh.

## Using it

Open **Settings → Remote computers**. Under **This computer**, enable remote
access on the computer that will run tasks, then choose **Show pairing code**.
On the controller, use **Connect to another computer → Find computers**, choose
**Enter code** beside that computer, and enter its code before choosing
**Pair computer**. Code entry appears only for the selected computer; canceling,
refreshing discovery, or selecting a different computer clears it. Codes are
shown in eight individual boxes grouped as `ABCD-EFGH`. The single underlying
field supports typing, selection and paste with or without the dash. Filling it
does not submit a pairing attempt automatically. Both must
be signed into the same Maple account and deployment/channel. Account login
alone does not grant access. The code expires in five minutes and permits one
attempt. A failed attempt requires a new, locally opened window.

Paired computers appear alongside local projects in the sidebar and project
menus. Each group is **computer · folder**, with its tasks beneath it; a globe
marks remote folders. The sidebar only shows folders with visible tasks; empty
folders remain available in the project menus. Selecting a task or project also
selects its execution computer. Select a **This computer** project to return to
local work. Local-only installations retain the existing sidebar layout. In Remote computers settings,
**Rename** changes the computer's label on this desktop; it does not change its
identity or pairing grant.

Each computer loads independently. The desktop first reads project/task
summaries and opens a transcript only when that computer is used. Cached groups
stay visible during a disconnect, alongside the connection status. Runs and task
updates arrive through the existing event stream; a small folder-list refresh
every thirty seconds also discovers empty projects selected by another desktop.
This is an in-memory navigation cache, not offline transcript replication. Search,
sidebar position and each computer's composer remain intact when switching.

Choose an existing remote project or enter its absolute path; there is no remote
filesystem browser. Images picked
on the controller are explicit uploads. Remote frames are limited to 16 MiB,
including JSON/base64 overhead; reduce attachments if a send exceeds that bound.
Microphone, playback, billing and account operations stay on the controller.
The Integrations pane manages the selected execution computer and labels it;
account and application preference panes still use this computer. Native
permission setup must be performed on the execution computer.

Closing a window with remote access enabled keeps the application available
(macOS Dock; Windows/Linux tray Open/Quit). Explicit Quit stops that computer's
local daemon and drains its work. Closing a controller view does not cancel work
on another computer. Revocation and disabling access take effect locally even
when discovery is unavailable; re-enabling does not restore revoked grants.

CLI equivalents: `remote status`, `remote enable`, `remote disable`,
`remote discover`, `remote begin-pairing`, `remote cancel-pairing`,
`remote pair <endpoint>` (hidden code prompt), and `remote revoke <endpoint>`.
These administer only the local computer. Never put a pairing code in argv or logs.

## Transport and recovery

Iroh 1.3.0 supplies authenticated QUIC, direct connectivity and relay fallback.
The minimal preset disables public address lookup and router port mapping.
There is no second encryption tunnel or custom routing layer. The remote
protocol uses `maple-agent/remote/1`, a simple minimum-version contract independent
of application builds, bounded frames and an explicit task-operation allowlist.
Account operations and whole-host administration cannot be forwarded.

Four pre-admission handshakes and eight paired control connections have capacity
separate from local IPC. Each control connection bounds ordinary calls and keeps
separate capacity for cancellation/questions. Slow readers lose their stream and
recover from an authoritative snapshot. Accepted mutations finish on the host;
transport loss never causes an automatic prompt replay. A stable send identity
crosses both IPC and Iroh; only the execution daemon records its admission and
result, account-bound in its receipt database.

The controller retries transient connection loss with bounded exponential delay
while the view remains attached. Known revocation and incompatible/unknown wire
messages terminate recovery. Recent validated address hints can be reused during
a discovery outage, but a daemon restart or new relay admission can still require
OpenSecret. No offline history replica or migration of workspaces is provided.

## Configuration and release boundary

`MAPLE_REMOTE_RELAYS` accepts an explicit comma-separated HTTPS relay list. An
empty list fails closed with a configuration error, with no public fallback.
The initial change does not invent or deploy a Maple relay hostname; the approved
Dev relay must be configured before a packaged remote trial. The ordinary
application remains usable with remote access off.

Unpackaged validation builds additionally accept `MAPLE_REMOTE_RELAYS=public-test`
and `MAPLE_REMOTE_RELAY_ONLY=1`. The latter removes IP transports, proving a real
relay path even for two processes on one machine. `MAPLE_REMOTE_RELAYS=none` is
available for direct-only development. Never ship public-test as a release default.
The execution and controller profiles need separate XDG config/data roots and
proxy ports when testing on one machine. Do not reuse installed production state.

The encryption/enrollment contract and dependency qualification are in
[remote-security.md](remote-security.md). The backend relay callback and stock
relay deployment inputs are documented in
[OpenSecret remote enrollment](../../../services/opensecret/docs/remote-enrollment.md).
Infrastructure deployment, signed/native-permission validation, Windows/Linux
tray testing and two real machines remain distinct release gates.

See [implementation validation](remote-validation.md) for observed behavior and remaining trial gates.
