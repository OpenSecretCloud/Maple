# List available commands
default:
    @just --list

# Start the frontend development server
dev:
    cd frontend && bun run dev

# Test the frontend (needs local backend running)
test:
    cd tests && bun test 

format:
    cd frontend && bun run format
