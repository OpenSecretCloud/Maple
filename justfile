# List available commands
default:
    @just --list

# Install frontend dependencies
install:
    cd frontend && bun install

# Start the frontend development server
dev:
    cd frontend && bun run dev

build:
    cd frontend && bun run build

# Test the frontend (needs local backend running)
test:
    cd tests && bun test 

format:
    cd frontend && bun run format
