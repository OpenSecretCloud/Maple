# Offline equation assets

KaTeX 0.18.10 (MIT), https://github.com/KaTeX/KaTeX/tree/v0.18.10.
`LICENSE` is the upstream license shipped with the npm distribution.
The fonts are the unmodified KaTeX fonts in that distribution. Their embedded
notices and the SIL Open Font License 1.1 are reproduced in `FONT-LICENSE.txt`.

Unmodified minified JS, CSS and woff2 font files were extracted from the official
npm distribution, https://registry.npmjs.org/katex/-/katex-0.18.10.tgz.
Tarball SHA-512 (base64):
`/B6p9eY9DX7aHBfpkHdpirDTZ5QH9xTwL9y837PWuzuv41O5jFdqNQwVQEQsimY0/iVNkwBY4+4hMfhQ7d4Dbw==`.
Tarball SHA-256:
`ab056aae3031254cdffc64e7b886cdc43c91114b219bd7b1b68e097b8b3da6c2`.

Only equations enter the ephemeral renderer. Markdown and transcript selection
remain native. The app allows no network, navigation, credentials, storage,
script messages, or renderer-controlled resource paths. `renderer.js`,
`renderer.css`, and `index.html` are the app-owned integration.
