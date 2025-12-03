# Maple Project Guidelines

Maple is a **Tauri-based AI chat application** that runs on desktop (macOS, Linux), iOS, and Android.

## Project Structure
- `frontend/` - React + TypeScript frontend (Vite)
- `frontend/src-tauri/` - Rust backend (Tauri)
- `frontend/.env.local` - Environment variables (API keys, URLs)

## Key Files

### Frontend
- `src/app.tsx` - App entry, sets up all providers (OpenSecret, QueryClient, etc.)
- `src/components/UnifiedChat.tsx` - **Main chat interface** (the logged-in experience)
- `src/state/LocalStateContext.tsx` - Global state (chats, models, billing status)
- `src/ai/OpenAIContext.tsx` - OpenAI API integration
- `src/utils/platform.ts` - Platform detection (iOS/Android/macOS/desktop/web)
- `src/billing/billingApi.ts` - Subscription and billing logic
- `src/routes/` - TanStack Router file-based routing (`_auth.*` routes require login)

### Rust (src-tauri)
- `src/lib.rs` - Tauri entry point, plugin setup, command handlers
- `src/proxy.rs` - Local proxy server for AI API calls (maple-proxy crate)
- `src/pdf_extractor.rs` - PDF text extraction

## Tech Stack
- **Frontend**: TypeScript + React (Vite), Tailwind CSS, shadcn/ui components
- **Backend**: Rust (Tauri 2.x) with proxy and PDF extraction features
- **Auth**: OpenSecret (`@opensecret/react`)
- **Package Manager**: Bun (v1.2.2+)

## Environment
The user typically runs `nix develop` (using `flake.nix`) before starting Claude. This means you're usually in a Nix shell with all required tools (bun, cargo, rustc, etc.) already available.

## Build & Development Commands
Use justfile for all commands (`just --list` to see all):

### Frontend (TypeScript/React)
- `just dev` - Start development server
- `just build` - Build for production
- `just format` - Format code with Prettier
- `just lint` - Run ESLint
- Always run `just format`, `just lint`, and `just build` after you're done with TypeScript/React changes

### Tauri Builds
- `just ios-dev` - iOS simulator (default)
- `just ios-dev-sim "iPhone 16 Pro"` - iOS specific simulator
- `just ios-dev-device "Your iPhone"` - iOS physical device
- `just android-build` - Android release build
- `just desktop-build` - Desktop release build
- `just desktop-build-no-cc` - Desktop build (with CC unset for compatibility)

### Rust (src-tauri)
- `just rust-fmt` - Format Rust code
- `just rust-check` - Check Rust compiles
- `just rust-clippy` - Run Clippy lints
- `just rust-lint` - Run all Rust checks (fmt + clippy strict)
- Always run `just rust-lint` after modifying Rust code in `src-tauri/`

## Code Style Guidelines

### TypeScript/React
- **Imports**: Use path aliases (`@/*` maps to `./src/*`)
- **Formatting**: 2-space indentation, double quotes, 100-char line limit
- **Types**: Strict TypeScript, avoid `any`
- **Naming**: PascalCase for components, camelCase for variables/functions
- **Components**: Functional components with hooks
- **State**: React context for global state, TanStack Query for server state
- **UI**: Use existing shadcn/ui components from `src/components/ui/`

### Rust
- Follow standard Rust conventions (rustfmt defaults)
- Use `log::info!`, `log::error!` for logging
- Tauri commands use `#[tauri::command]` attribute
- Handle errors with `anyhow` or explicit Result types

## Git Workflow
- **Never commit directly to master** - always create a new branch for your work
- **Don't commit until changes are tested and confirmed working**
- **Amend commits** only for minor fixes directly related to the previous commit (forgotten files, typos, direct bugs from that commit)
- **Create new commits** for new functionality or features, even if related to recent work
- When the feature is complete and verified, push the branch to origin and create a PR using `gh pr create`
