# Maple updater pointer Worker

This Worker serves one public endpoint:

```text
GET https://updates.trymaple.ai/latest.json
```

Every other path returns `404`, and methods other than `GET` and `HEAD` return
`405`. When `public/latest.json` is absent, the endpoint returns `404` so Tauri
continues to the GitHub updater endpoint. A present file is returned only after
its stable version, release timestamp, required platform entries, signatures,
and GitHub Maple release URLs validate. Invalid or unavailable metadata returns
`503` so clients retain the GitHub fallback.

The live pointer is generated and deployed by release automation. It is ignored
by git and must not be committed. The checked-in Worker has no production route,
`workers.dev` endpoint, or preview URL; attaching the custom domain is a separate
authorized Cloudflare operation.

## Development

Use Maple's pinned Nix shell and the lockfile in this directory:

```sh
nix develop --no-update-lock-file .#ci -c ./scripts/ci/updates.sh
```

For local iteration:

```sh
cd updates
bun install --frozen-lockfile --ignore-scripts
bun run test
bun run dev
```

`bun run check` formats, typechecks, tests, and performs a credential-free
Wrangler dry-run bundle. `bun run deploy` is a production action; do not run it
without explicit authority and the intended Cloudflare account, route, and
pointer state.
