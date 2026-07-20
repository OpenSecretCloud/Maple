# Agent Mode MCP support

This document describes Maple's first Model Context Protocol (MCP) integration in Agent Mode. The implementation intentionally follows the thin custom-extension experience in Goose Desktop and uses Maple's embedded Goose SDK directly.

## MVP behavior

Agent Mode supports user-configured MCP servers over:

- Standard input/output (STDIO)
- Streamable HTTP

The global MCP manager lets a user add, edit, delete, and choose default servers. The composer's MCP menu can search the configured list. Each definition has a name, optional description, positive timeout in seconds, and transport-specific settings:

- STDIO: one command field plus optional environment variables
- Streamable HTTP: endpoint URL, optional environment variables, and optional request headers

The STDIO command is tokenized into an executable and argument list. It is never passed to a shell. Saving a definition does not test or start it.

Enabled defaults are copied into a new Agent session. Changing, renaming, disabling, or deleting a global definition affects future sessions only. The puzzle menu in the composer controls the current session: enabling connects immediately and persists only after a successful connection; disabling disconnects immediately and removes the server from that session's Goose snapshot.

A connection failure is nonfatal. The session still opens and Maple reports the failed server. Goose persists only servers that connected successfully.

## Persistence and security boundary

MCP definitions are account-scoped in Maple's Agent Mode `config.json`, which is written with owner-only permissions on Unix systems. Existing Agent sessions store their selected extensions in that account's Goose session metadata.

Environment and HTTP header values are stored in the account definition and copied into each selected session's extension snapshot. Freezing the complete configuration prevents a later credential change from being sent to an older session's endpoint or process. It also avoids Goose's process-global secret store, which cannot isolate the same environment-variable name by Maple account or session.

The account directory is owner-only on Unix systems, but these values are not encrypted at rest. The UI obscures them while editing; obscuring a field is not encryption. Deleting an Agent session removes its snapshot, and clearing the account's local Agent data removes both its definitions and session store.

Maple rejects Goose's disallowed process-overriding variables, empty or duplicate keys, duplicate case-insensitive HTTP header names, names that collide after Goose normalization, and the reserved `developer` and `maple-skills-extension` extension names.

## Compatibility boundary

Maple currently inherits the MCP implementation and protocol negotiation from its pinned Goose SDK. At the time of this MVP, Goose advertises MCP revision `2025-03-26`. A modern `@modelcontextprotocol/server-everything` release using TypeScript SDK 1.29.0 was verified to negotiate that revision successfully over both supported transports.

This MVP does not include:

- Legacy HTTP+SSE transport
- Built-in or curated MCP servers
- A server catalog, import/export, deep links, or install recipes
- MCP connection tests, health history, or reconnect controls
- Tool allowlists, MCP resources/prompts UI, Unix sockets, or custom working directories
- Maple-owned OAuth configuration or guarantees
- A Maple-owned sampling or elicitation implementation

Goose may automatically attempt browser OAuth after an HTTP authentication challenge. Maple does not expose controls for that inherited behavior, and Goose's OAuth credentials are not durable across Maple Agent runtime restarts. For this MVP, supported authentication is limited to STDIO environment variables and static Streamable HTTP headers (including headers that reference configured environment variables).

## Deterministic smoke test

Use the official Everything test server pinned to `2026.1.14`.

STDIO definition:

```text
Name: fixture_stdio
Command: npx --offline -y @modelcontextprotocol/server-everything@2026.1.14 stdio
Timeout: 30
```

Remove `--offline` if the package is not already cached.

For Streamable HTTP, start the server separately:

```sh
PORT=33001 npx --offline -y \
  @modelcontextprotocol/server-everything@2026.1.14 streamableHttp
```

Then configure:

```text
Name: fixture_http
Endpoint: http://127.0.0.1:33001/mcp
Timeout: 30
```

For each transport, enable only that fixture in a new Agent session and ask Maple to call its prefixed `echo` tool with a unique marker. Confirm the visible tool request, arguments, tool result, and final answer all contain the same marker. Then verify that disabling it removes the tool from that session and that a stopped HTTP server produces a visible connection error without disabling Maple's built-in developer tools.

## Follow-up direction

Keep the compatibility policy conservative: support what the pinned Goose SDK interoperates with today, and change the advertised MCP revision or fork Goose only in response to a demonstrated server incompatibility. Likely follow-ups are a curated privacy-oriented catalog, durable and account-isolated OAuth, a stronger secret store for definitions and headers, server health/reconnect UX, and richer MCP capability surfaces.
