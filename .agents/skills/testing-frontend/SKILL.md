# Testing Maple Frontend

## Setup

1. Start the dev server from the `frontend/` directory:
   ```bash
   cd frontend && npx vite --host 0.0.0.0
   ```
   The dev server runs on `http://localhost:5173`.

2. Log in with test credentials (stored as Devin secrets: `MAPLE_TEST_EMAIL` and `MAPLE_TEST_PASSWORD`).

## Key Routes

- `/` - Main chat page with sidebar (history, new chat)
- `/settings` - Settings page with dedicated sidebar
- `/settings?tab=profile` - Profile tab
- `/settings?tab=subscription` - Subscription tab
- `/settings?tab=api` - API Management tab
- `/settings?tab=team` - Team Management tab (only visible for paid plans)
- `/settings?tab=data` - Data & Privacy tab
- `/settings?tab=about` - About tab

## Settings Page Layout

- **Desktop**: 280px sidebar on the left with settings navigation, "Back to Chat" and "Log out" in footer. Main content area on the right.
- **Mobile**: Horizontal scrollable tabs at the top, logout button at bottom of content area.

## Build & Lint

Always run these after making frontend changes:
```bash
cd frontend && npx tsc --noEmit    # TypeScript check
cd frontend && npx prettier --write src/  # Format
```

Or use the project's `just` commands:
```bash
just format
just lint
just build
```

## Notes

- The test account is on a free plan, so Team Management tab is hidden.
- Billing status is fetched via React Query on the settings page.
- The app uses TanStack Router for routing.
- UI components are from shadcn/ui (`src/components/ui/`).
