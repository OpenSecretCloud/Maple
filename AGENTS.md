# Maple Development Guide

This repository contains the Maple AI assistant frontend - a cross-platform desktop and mobile application built with Tauri, React, and TypeScript.

## Tech Stack
- **Frontend**: React 19, TypeScript, TanStack Router, TanStack Query
- **UI**: Tailwind CSS, shadcn/ui components
- **Desktop**: Tauri (Rust backend)
- **Build**: Bun for package management and bundling
- **Mobile**: iOS and Android via Tauri mobile

## Key Directories
- `frontend/src/components/` - React components
- `frontend/src/routes/` - TanStack Router routes
- `frontend/src/state/` - State management contexts
- `frontend/src-tauri/` - Rust Tauri backend
- `docs/` - Technical documentation

## Common Development Tasks

**Run development server**:
```bash
bun run dev          # Web only
bun tauri dev        # Desktop app
```

**Build**:
```bash
just desktop-build   # Production desktop build
bun tauri build      # Direct Tauri build
```

**Format & Test**:
```bash
just format          # Format code
just test            # Run tests
```

See `README.md` for complete development setup, build instructions, and platform-specific notes.

## Code Quality Standards
- TypeScript strict mode enabled
- Follow existing component patterns (see `frontend/src/components/`)
- Use TanStack Query for server state
- Use context for client state (see `frontend/src/state/`)
- Tailwind CSS for styling (custom Maple design tokens in `--maple-*` CSS variables)
- Accessibility: proper ARIA labels, keyboard navigation, semantic HTML

## Testing Changes
- For UI changes, run the dev server and manually test
- Test both light and dark modes
- Check mobile responsive layouts
- Verify keyboard navigation and screen reader support

## Git Workflow
- Work on feature branches (e.g., `agent/maple-<feature>`)
- Commit with clear, descriptive messages
- Open PRs for review (do not merge to master directly)
- Run pre-commit hooks (see `setup-hooks.sh`)

## State Management Patterns
- **Server state**: Use TanStack Query (`useQuery`, `useMutation`)
- **Local UI state**: Use React state (`useState`, `useReducer`)
- **Shared client state**: Use context (see `LocalStateContext`, `ChatRuntimeContext`)
- **Persistence**: localStorage for user preferences, IndexedDB for larger data

## Platform-Specific Code
Check platform with utilities from `frontend/src/utils/platform.ts`:
```typescript
import { isTauri, isMacOS, isLinux, isIOS, isAndroid } from "@/utils/platform";
```

## Security
- Never commit sensitive keys or credentials
- API URLs via environment variables (`VITE_OPEN_SECRET_API_URL`)
- User data encrypted via OpenSecret SDK
- Tauri security best practices (CSP, allowlist)

## Key Documentation
- `README.md` - Development setup, build instructions, releases
- `MAPLE_UX_CONTEXT.md` - UI feature reference for understanding the Maple app interface
- `docs/` - Technical specifications and architectural decisions
