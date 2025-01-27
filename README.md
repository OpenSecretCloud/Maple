# Maple AI Frontend

Uses [bun](https://bun.sh/) for development.

```bash
bun install
bun run dev
```

Expects a `VITE_OPEN_SECRET_API_URL` environment variable to be set. (See `.env.example`)

## Updating PCR0 values

If there's a new version of the enclave pushed to staging or prod, append the new PCR0 value to the `pcr0Values` or `pcr0DevValues` arrays in `frontend/src/app.tsx`.
