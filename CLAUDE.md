# Maple Project Guidelines

## Build & Development Commands
- `bun run format` - Format code with Prettier
- `bun run lint` - Run ESLint
- `bun run build` - Build for production
- Always run the above commands when you get done with code changes and fix any errors.

## Code Style Guidelines
- **Imports**: Use path aliases (e.g., `@/*` maps to `./src/*`)
- **Formatting**: 2-space indentation, double quotes, 100-char line limit
- **Types**: Use strict TypeScript typing, no `any` when avoidable
- **Naming**: PascalCase for components, camelCase for variables/functions
- **Error Handling**: Use try/catch with specific error types
- **Components**: React functional components with hooks
- **State Management**: Use React context for global state when needed

## Tech Stack
- TypeScript + React (Vite)
- Tailwind CSS for styling
- Playwright for testing
- Bun package manager (v1.2.2+)
