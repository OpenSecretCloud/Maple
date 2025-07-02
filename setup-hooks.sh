#!/bin/sh

echo "Setting up git hooks..."

# Get the root directory of the git repository
GIT_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)

if [ -z "$GIT_ROOT" ]; then
    echo "Error: Not in a git repository"
    exit 1
fi

# Set git to use our hooks directory
git config core.hooksPath .githooks

echo "✅ Git hooks configured successfully!"
echo "The pre-commit hook will now:"
echo "  1. Check code formatting with 'bun run format:check'"
echo "  2. Run 'bun run build' to ensure the project builds"