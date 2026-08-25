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

The live pointer is ignored by git and must not be committed. The protected
`Publish updater metadata` GitHub Actions workflow downloads the current stable
GitHub Release's `latest.json`, verifies its GitHub digest and Worker schema,
deploys it, and confirms that the public endpoint serves the exact same bytes.
The first run also creates the `updates.trymaple.ai` Custom Domain and its DNS and
TLS configuration. Publication is manual until the release workflow is connected
in a later phase. Do not use a local Wrangler login for production deployment.

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
