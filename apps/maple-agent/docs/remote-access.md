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
attempt. A failed attempt requires a new, locally opened window. Leaving Settings
keeps the current window alive; returning shows the same code and remaining time,
with Cancel available. Navigation never renews its deadline or attempt.

Paired computers appear alongside local projects in the sidebar and project
menus. Each group is **computer · folder**, with its tasks beneath it; a globe
marks remote folders. The sidebar only shows folders with visible tasks; empty
folders remain available in the project menus. Selecting a task or project also
selects its execution computer. Select a **This computer** project to return to
local work. Local-only installations retain the existing sidebar layout.

The computer name defaults to its OS hostname. In **Remote computers → Computer
name → Rename**, set the name advertised by this computer for this account and
profile. This works with remote access off and does not change the system
hostname. Other paired computers learn changes when refreshing discovery or
reconnecting; the name is not broadcast over active task streams. **Nickname**
on a paired row overrides its label on the viewing desktop only. Neither name
changes device identity or pairing authority.

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
(macOS Dock; Windows/Linux tray Open/Quit). On a desktop without a usable tray,
Close offers Keep open or Quit Maple, explaining that Quit stops remote access.
Explicit Quit stops that computer's
local daemon and drains its work. Closing a controller view does not cancel work
on another computer. Revocation and disabling access take effect locally even
when discovery is unavailable; re-enabling does not restore revoked grants.

CLI equivalents: `remote status`, `remote enable`, `remote disable`,
`remote discover`, `remote begin-pairing`, `remote cancel-pairing`,
`remote pair <endpoint>` (hidden code prompt), and `remote revoke <endpoint>`.
These administer only the local computer. Never put a pairing code in argv or logs.

## Transport and recovery

Iroh 1.3.0 supplies authenticated QUIC, direct connectivity and relay fallback.
Maple explicitly clears public address lookup and builds Iroh without its
port-mapping feature. The Minimal preset alone is not the port-mapping policy.
There is no second encryption tunnel or custom routing layer. The remote
protocol keeps the `maple-agent/remote/1` ALPN family and requires compatible wire version 4 or newer, independent of application builds. Older peers receive an explicit
version refusal. Bounded frames and an explicit task-operation allowlist apply.
Account operations and whole-host administration cannot be forwarded.

Four pre-admission handshakes and eight paired control connections have capacity
separate from local IPC. Each control connection bounds ordinary calls and keeps
separate capacity for cancellation/questions. Each request/reply uses an
independent bidirectional QUIC stream; the authenticated initial stream carries
ordered events. A bounded header admits each request before its attachment body
is read. Stop and question answers have reserved capacity and higher native stream
priority, so an incomplete upload or unread image reply cannot hold their dispatch
loop. Streams still share the connection's bandwidth; this is not a latency
guarantee on a saturated or unavailable network. Transfers use bounded frames and
inactivity deadlines, allowing slow transfers that keep making progress. Image
readback uses base64 on the remote wire.
A request failure or oversized reply returns a request error, while authority loss
or incompatible framing ends the connection. A failed connection recovers from an
authoritative snapshot. Transport loss does not cancel an accepted mutation on
the host or cause an automatic prompt replay; explicit host shutdown still stops
its work. A stable send identity
crosses both IPC and Iroh; only the execution daemon records its admission and
result, account-bound in its receipt database. An uncertain send keeps its original
identity, text and draft images in the desktop view and queries the execution
host's receipt after reconnecting. It never replays the command or overwrites a
newer draft. If the execution daemon restarts before recording a send's result,
it marks that receipt interrupted, preserving the identity against replay. The
message may already have taken effect: the desktop says so, retains/restores its
draft without overwriting newer input, and lets the user continue the task. A
pending receipt from a live daemon still blocks another send until resolved.
This is in-memory draft recovery, not persistence across desktop crashes.

A remote-enabled daemon retries transient startup failures with bounded backoff.
Local settings show whether networking is ready and retain actionable configuration
errors. Disable and revoke invalidate initialization already in flight; their local
effect does not wait for discovery publication.

After an abnormal exit, an older local daemon can still own this profile when an
updated application starts. The startup error offers a user-confirmed
Stop-and-reopen action; it stops the old host's tasks and remote access before
starting the new build. There is no automatic takeover.

The controller retries transient connection loss with bounded exponential delay
while the view remains attached. Known revocation and incompatible/unknown wire
messages terminate recovery. Recent validated address hints can be reused during
a discovery outage, but a daemon restart or new relay admission can still require
OpenSecret. No offline history replica or migration of workspaces is provided.

## Configuration and release boundary

Packaged Dev and Prod builds use Iroh's standard n0 public relays for the initial
testing rollout. No Maple-operated relay or relay callback credential is required
for that trial. OpenSecret must still be deployed with remote enrollment support
and its database migration in the matching environment. Remote access remains
off on a fresh profile; the default does not start networking by itself. Public
relays do not enable public discovery or replace Maple account admission, pairing
or local grants.

`MAPLE_REMOTE_RELAYS` overrides that default with an explicit comma-separated HTTPS
relay list. As with other environment settings, unset or blank means use the
profile default. An invalid nonempty override fails with a configuration error
rather than falling back to public relays. Maple-operated relays can be selected
with this override when available.

Unpackaged validation builds additionally accept `MAPLE_REMOTE_RELAYS=public-test`
and `MAPLE_REMOTE_RELAY_ONLY=1`. The latter removes IP transports, proving a real
relay path even for two processes on one machine. `MAPLE_REMOTE_RELAYS=none` is
available for direct-only development. These shortcuts remain unpackaged-only;
an unpackaged build without explicit relay configuration still reports an error.
The execution and controller profiles need separate XDG config/data roots and
proxy ports when testing on one machine. Do not reuse installed production state.

The encryption/enrollment contract and dependency qualification are in
[remote-security.md](remote-security.md). The backend relay callback and stock
relay deployment inputs are documented in
[OpenSecret remote enrollment](../../../services/opensecret/docs/remote-enrollment.md).
Infrastructure deployment, signed/native-permission validation, Windows/Linux
tray testing and two real machines remain distinct release gates.

See [implementation validation](remote-validation.md) for observed behavior and remaining trial gates.
