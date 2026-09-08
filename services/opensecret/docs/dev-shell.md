# Development shell

`nix develop` provides the pinned toolchain and manages optional local state.
Before starting a cluster, its hook checks `localhost:$PGPORT`; if a server
responds, that listener is reused even when it does not belong to this
checkout. Otherwise the hook initializes or starts `$PGDATA`. It creates `.env`
from `.env.sample` only when `.env` is absent.

## Shell-hook controls

| Variable | Effect |
| --- | --- |
| `OPENSECRET_DEV_POSTGRES=0` | Do not inspect, initialize, or start PostgreSQL. |
| `OPENSECRET_DEV_ENV=0` | Do not create `.env`. |
| `OPENSECRET_DEV_CONTAINERS=0` | Do not configure Linux user-level container state. |
| `PGDATA` / `PGSOCKETS` / `PGPORT` | Select local PostgreSQL state, sockets, and listener. |
| `OPENSECRET_DEV_DATABASE_URL` | Set the database URL written into a newly generated `.env`. |
| `OPENSECRET_BIND_ADDR` | Select the backend listener instead of `127.0.0.1:3000`. |

For a pure check, disable every stateful hook and avoid changing the lockfile:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c cargo fmt --all -- --check
```

For concurrent live checkouts, choose distinct `PGDATA`, `PGSOCKETS`, `PGPORT`,
`DATABASE_URL`, and backend bind addresses. Verify the database identity before
running migrations or destructive tests; never assume that a responding port
belongs to the current checkout.

## Logging

`APP_MODE=local` writes line-buffered tracing to stdout so redirected `cargo run`
logs appear immediately. When `RUST_LOG` is unset the default is
`opensecret=debug` plus `axum_login`, `tower_sessions`, `sqlx=warn`, and
`tower_http`. Override `RUST_LOG` to quiet or expand that set. Do not log
secrets, tokens, decrypted bodies, or raw provider payloads.
