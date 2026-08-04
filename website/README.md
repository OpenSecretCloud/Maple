# Website

This website is built using [Docusaurus](https://docusaurus.io/), a modern static website generator.

Use Bun 1.3.5 for this repository. The Bun lockfile is authoritative; do not
update website dependencies with npm or Yarn.

### Installation

```
$ bun install
```

### Local Development

```
$ bun run start
```

This command starts a local development server and opens up a browser window. Most changes are reflected live without having to restart the server.

### Build

```
$ bun run build
```

This command generates static content into the `build` directory and can be served using any static contents hosting service.

### Deployment

Using SSH:

```
$ USE_SSH=true bun run deploy
```

Not using SSH:

```
$ GIT_USER=<Your GitHub username> bun run deploy
```

If you are using GitHub pages for hosting, this command is a convenient way to build the website and push to the `gh-pages` branch.
