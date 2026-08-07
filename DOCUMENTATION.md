# Maple Documentation Guide

This document explains the purpose of each documentation file in this repository and how they work together to provide context for both developers and the Maple AI assistant.

## Documentation Files

### For Developers

#### `README.md`
**Purpose**: Primary developer documentation  
**Audience**: Engineers working on the Maple codebase  
**Content**:
- Development setup and prerequisites
- Build instructions for all platforms
- Release process and versioning
- Testing and deployment

**When to read**: Starting development, building releases, troubleshooting setup

#### `AGENTS.md`  
**Purpose**: Quick reference for AI agents working on the codebase  
**Audience**: AI assistants (Claude, etc.) in development mode  
**Content**:
- Tech stack overview
- Key directories and file structure
- Common development commands
- Code quality standards
- Git workflow

**When loaded**: Automatically by AI development tools (e.g., Claude Code) when working on this repository

### For Maple AI Assistant Context

#### `MAPLE_SYSTEM_INSTRUCTION.md`
**Purpose**: Concise system instruction for the Maple assistant  
**Audience**: Maple AI when assisting end-users in conversations  
**Content**:
- Interface detection rules (app vs API/proxy)
- Core UI features (web search, uploads, projects)
- When to reference UI vs capabilities
- Concise guidance for common scenarios

**How to use**: 
1. Users can add this as a custom system prompt in Maple Settings → Preferences → "Default system prompt"
2. Copy the content into the system prompt field
3. Save preferences
4. The instruction will be included in all new conversations

**Format**: Designed to be copy-pasted directly into the preferences UI

#### `MAPLE_UX_CONTEXT.md`
**Purpose**: Comprehensive technical reference for Maple's UI  
**Audience**: AI assistants, developers needing detailed UI implementation info  
**Content**:
- Detailed feature descriptions with code references
- Implementation details and file locations
- Visual states and styling information
- Platform-specific differences
- Feature availability matrix

**When to read**: 
- When assistant needs detailed UI information
- When updating UI features (to keep docs current)
- When debugging UI-related issues

**Not loaded automatically**: Too large for system prompt; used as reference documentation

#### `docs/` Directory
**Purpose**: Technical specifications and architectural decisions  
**Content**: Detailed implementation docs for specific features
- `product-redesign-spec.md`
- `unified-chat-refactor.md`
- `pdf-ocr.md`
- `conversations-api-implementation.md`
- And more...

## How They Work Together

### Development Scenario
1. Developer opens the Maple repo
2. AI assistant reads `AGENTS.md` (lightweight, automatically loaded)
3. For detailed UI info, assistant references `MAPLE_UX_CONTEXT.md`
4. For specific features, assistant reads relevant `docs/*.md` files

### User Conversation Scenario

**Option 1: User adds system instruction manually**
1. User opens Maple Settings → Preferences
2. User copies content from `MAPLE_SYSTEM_INSTRUCTION.md`
3. User pastes into "Default system prompt" field
4. User saves preferences
5. All new conversations include this context

**Option 2: Default installation (future)**
- Maple could ship with `MAPLE_SYSTEM_INSTRUCTION.md` pre-loaded as a default instruction
- Would require backend changes to include it in system prompt automatically

### Agent Mode (Desktop App)
- Agent Mode uses Goose's skills system
- Automatically discovers `.claude/` directory in project roots
- `AGENTS.md` in a project root is loaded as development context
- No manual configuration needed

## File Size Considerations

| File | Size | Purpose | Loading |
|------|------|---------|---------|
| `AGENTS.md` | ~2KB | Quick dev reference | Auto (AI tools) |
| `MAPLE_SYSTEM_INSTRUCTION.md` | ~3KB | Concise user context | Manual copy-paste |
| `MAPLE_UX_CONTEXT.md` | ~15KB | Detailed UI reference | Reference only |
| `README.md` | ~12KB | Full dev guide | Manual read |

**System Prompt Budget**: Most LLM system prompts have ~8-16KB budget. `MAPLE_SYSTEM_INSTRUCTION.md` fits comfortably, while `MAPLE_UX_CONTEXT.md` is reference material consulted as needed.

## Maintaining These Files

### When to Update

**Update `MAPLE_SYSTEM_INSTRUCTION.md` when**:
- Core UI features are added/removed/moved
- Default behaviors change (e.g., web search default)
- New platform support added
- User-facing capabilities change

**Update `MAPLE_UX_CONTEXT.md` when**:
- Implementation details change
- Code references need updating (file paths, line numbers)
- Visual states or styling changes
- Platform-specific behavior changes
- New settings sections added

**Update `AGENTS.md` when**:
- Tech stack changes
- Build process changes
- New development patterns emerge
- Git workflow changes

### How to Update

1. **Test the actual UI** in a development build
2. **Verify code references** (file paths, line numbers, component names)
3. **Update the relevant file(s)**
4. **Check consistency** between MAPLE_SYSTEM_INSTRUCTION.md (concise) and MAPLE_UX_CONTEXT.md (detailed)
5. **Commit with clear description** of what changed and why

### Version Tracking

These docs reflect the Maple UX as of **August 2024**. Consider adding version markers when making significant updates:

```markdown
<!-- Last updated: September 2024 - Added new feature X -->
```

## Future Improvements

**Potential enhancements**:
1. **Auto-loading system instruction**: Backend support to include `MAPLE_SYSTEM_INSTRUCTION.md` automatically
2. **Versioned documentation**: Track docs against Maple version releases
3. **Interactive documentation**: In-app help that references these guides
4. **Automated updates**: Scripts to extract UI structure from codebase
5. **Documentation tests**: Verify code references are still accurate

## Questions?

- **For development**: See `README.md` or ask in the project repository
- **For UI features**: See `MAPLE_UX_CONTEXT.md` for detailed technical info
- **For user guidance**: See `MAPLE_SYSTEM_INSTRUCTION.md` for what to tell users
