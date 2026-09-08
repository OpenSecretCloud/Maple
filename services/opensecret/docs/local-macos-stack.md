# Local macOS stack

This runbook covers OpenSecret with the native Continuum proxy and the
in-process Tinfoil Rust SDK. It is separate from Linux/Nitro deployment.

```text
Continuum proxy   http://127.0.0.1:8092
OpenSecret API    http://127.0.0.1:3000
Maple             VITE_OPEN_SECRET_API_URL=http://127.0.0.1:3000
```

Tinfoil discovery, attestation, TLS pinning, and requests happen inside the
OpenSecret process; there is no local Tinfoil sidecar or port.

## One-time setup

```sh
git submodule update --init --recursive
install -d -m 700 .local/secrets
touch .local/secrets/tinfoil_api_key .local/secrets/continuum_api_key
chmod 600 .local/secrets/tinfoil_api_key .local/secrets/continuum_api_key
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c just build-local-proxies-macos
```

Populate the credential files without printing their contents. The generated
proxy binary and secret directory are gitignored local state.

Enter `nix develop` once to prepare the local PostgreSQL state and create `.env`
when absent. Review an existing `.env` rather than replacing it, then run:

```sh
just diesel-migration-run-local
```

## Run

Use separate terminals.

Terminal 1:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c just run-continuum-proxy-macos
```

Terminal 2:

```sh
nix develop --no-update-lock-file -c just run-local-backend-macos
```

Local backend logs are line-buffered on stdout. Follow that terminal, or the
capturing process's log file (workspace-managed starts use `logs/opensecret.log`).

The backend recipe selects the loopback Continuum base and reads the Tinfoil
credential. Any other custom provider base is a credential boundary; derive
URL and header behavior from current source before supplying credentials.

In Maple, set its ignored local configuration to:

```dotenv
VITE_OPEN_SECRET_API_URL=http://127.0.0.1:3000
```

Follow the selected Maple revision's own `AGENTS.md` and development or
validation skill when present. Browser Research and native Agent Mode are
separate consumers; choose the path that exercises the changed contract.
