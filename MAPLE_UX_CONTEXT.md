# Maple UI Context - Detailed Reference

This document provides comprehensive technical details about Maple's UI features. For the concise system instruction version, see `MAPLE_SYSTEM_INSTRUCTION.md`.

## Overview

Maple is a cross-platform AI assistant application with desktop (Tauri), web, and mobile interfaces. This guide documents the UI features available when users interact through the Maple app.

## Detection & Context Awareness

**Key Rule**: Only reference UI elements when the user is clearly in the Maple app.

**Indicators user is in Maple app**:
- Using desktop/mobile application
- Talking about app-specific UI ("I don't see the button", "where is the setting")
- No mention of API, proxy, CLI, or external integration

**Indicators user is NOT in Maple app**:
- Mentions "API", "proxy", "SDK", "CLI"
- Programmatic integration context
- Third-party client or tool

**When uncertain**: Ask directly: "Are you using the Maple desktop/mobile app, or accessing via API/proxy?"

## Core Chat Features

### Web Search Toggle

**Location**: Chat composer toolbar (bottom of chat interface)  
**Visual**: Globe icon  
**Code reference**: `frontend/src/components/UnifiedChat.tsx` lines ~5262-5283

**Functionality**:
- Toggles web search capability for the current and future messages
- Click to toggle between enabled/disabled states
- State persists to `localStorage` as `webSearchEnabled`

**Visual States**:
```tsx
// Active (enabled)
className: "text-[hsl(var(--maple-primary))]"

// Inactive (disabled)  
className: "text-[hsl(var(--maple-secondary-700))]"
```

**Default Behavior**:
- Web search is enabled by default for all users (`getInitialWebSearchEnabled()` returns `true`)
- User preference is stored in localStorage and respected on return visits
- One-time migration clears stale auto-persisted values (see `migrateWebSearchDefault()`)

**Implementation Details**:
```typescript
// From LocalStateContext.tsx
export function getInitialWebSearchEnabled(): boolean {
  migrateWebSearchDefault();
  const webSearchSetting = localStorage.getItem("webSearchEnabled");
  if (webSearchSetting !== null) {
    return webSearchSetting === "true";
  }
  return true; // Default to enabled
}
```

**When to mention**:
- User asks about current events, real-time data, or web information
- Suggesting they enable/disable for specific query types
- Explaining why an answer has or lacks web-sourced information

**Guidance examples**:
- ✅ "Web search is currently enabled (globe icon is highlighted). I can look up current information."
- ✅ "To disable web search, click the globe icon in your composer toolbar."
- ❌ (API user) "Click the globe icon..." → Instead: "Web search can be enabled in your API request"

### File Upload System

**Location**: Chat composer toolbar  
**Visual**: Plus (+) icon  
**Code reference**: `frontend/src/components/UnifiedChat.tsx` lines ~5285-5349

**Structure**: Dropdown menu with two options

#### Add Images

**Menu Item**: "Add Images" with Image icon  
**Code reference**: Lines 5316-5330 in UnifiedChat.tsx

**Availability**:
- Requires `canUseImages` (vision-enabled model)
- May require paid plan
- Disabled when `isGenerating` is true
- Shows upgrade dialog if user lacks access

**Supported Formats**: PNG, JPG, JPEG, WebP, and other standard image formats

**Implementation Flow**:
```typescript
onClick={() => {
  if (!canUseImages) {
    setUpgradeFeature("image");
    setUpgradeDialogOpen(true);
  } else {
    fileInputOwnerKeyRef.current = activeRuntimeKeyRef.current;
    fileInputRef.current?.click();
  }
}}
```

**Platform Support**:
- Desktop (Tauri): Full support
- Web: Full support
- Mobile: Full support with camera integration

#### Add Document

**Menu Item**: "Add Document" with FileText icon  
**Code reference**: Lines 5331-5347 in UnifiedChat.tsx

**Availability**:
- Desktop (Tauri): Full PDF support with OCR
- Web: Shows platform dialog directing to desktop app
- Requires `canUseDocuments` capability (may need paid plan)
- Disabled when `isGenerating` is true

**Implementation Flow**:
```typescript
onClick={() => {
  if (!isTauriEnv) {
    setDocumentPlatformDialogOpen(true);
  } else if (!canUseDocuments) {
    setUpgradeFeature("document");
    setUpgradeDialogOpen(true);
  } else {
    documentInputOwnerKeyRef.current = activeRuntimeKeyRef.current;
    documentInputRef.current?.click();
  }
}}
```

**OCR Processing**: Desktop app includes PDF OCR via ONNX Runtime (see `docs/pdf-ocr.md`)

**When to mention uploads**:
- User wants to share screenshots, photos, diagrams
- User mentions having a PDF or document to analyze
- Asking about image or document capabilities

**Guidance examples**:
- ✅ "You can upload images by clicking the + icon and selecting 'Add Images'."
- ✅ "PDF documents with OCR are supported on the desktop app. Click + → Add Document."
- ❌ (API user) "Click the + button..." → Instead: "You can include images in your API request"

### Project Picker

**Location**: Chat composer toolbar  
**Visual**: Folder icon  
**Code reference**: `frontend/src/components/ConversationProjectPicker.tsx`

**Functionality**:
- Organizes conversations into projects
- Projects are user-created via OpenSecret API
- Selection persists per conversation
- Helps group related chats together

**Visual States**:
```tsx
// No project selected
<Folder className="h-4 w-4" />  // Gray

// Project selected
<FolderOpen className="h-4 w-4" />  // Colored
className: "bg-[hsl(var(--maple-primary-container))] text-[hsl(var(--maple-primary))]"
```

**Implementation**:
```typescript
interface ConversationProjectPickerProps {
  selectedProjectId: string | null;
  onSelect: (projectId: string | null) => void | Promise<void>;
  disabled?: boolean;
}
```

**Data Source**:
```typescript
const { data: projects = [] } = useQuery({
  queryKey: ["conversationProjects", userId],
  queryFn: () => listAllConversationProjects(os),
  enabled: !!userId
});
```

**When to mention**:
- User organizing work across multiple projects
- Suggesting they group related conversations
- Explaining how to find conversations in specific project

**Guidance examples**:
- ✅ "You can organize this chat into a project using the folder icon."
- ✅ "Projects help group related conversations. Click the folder icon to select one."

## Model Selection

**Location**: Chat composer toolbar or conversation header  
**Component**: `ModelSelector` dropdown  
**Code reference**: `frontend/src/components/ModelSelector.tsx`

**Available Models** (as of current version):

```typescript
// From LocalStateContext.tsx
const DEFAULT_MODEL_ALIASES: OpenSecretModelAlias[] = [
  {
    id: QUICK_MODEL_ALIAS,  // "auto:quick"
    label: "Quick",
    description: "Fast, everyday responses",
    access: "free",
    capabilities: { chat: true, vision: false, reasoning: true, tool_use: true }
  },
  {
    id: POWERFUL_MODEL_ALIAS,  // "auto:powerful"  
    label: "Powerful",
    description: "Deeper thinking & analysis",
    access: "pro",
    capabilities: { chat: true, vision: true, reasoning: true, tool_use: true }
  }
];
```

**Default Model Logic**:
```typescript
function getInitialModel(): string {
  // 1. Dev override (VITE_DEV_MODEL_OVERRIDE)
  // 2. User's explicit choice (localStorage.getItem("selectedModel"))
  // 3. Paid defaults if already applied
  // 4. Check billing status for default
  // 5. Fall back to DEFAULT_MODEL_ID ("auto:quick")
}
```

**Paid User Defaults**:
- New paid users automatically get Powerful model + web search enabled
- Applied once, tracked via `localStorage.getItem("paidDefaultsApplied")`

**When to mention**:
- User wants faster responses → Quick model
- User needs vision/image analysis → Powerful model (required)
- User wants more thorough analysis → Powerful model

## Settings & Preferences

**Location**: App navigation menu → Settings  
**Routes**: `frontend/src/routes/settings.*.tsx`

### Settings Sections

#### 1. Preferences (`/settings/preferences`)
**Component**: `frontend/src/components/settings/PreferencesSettings.tsx`

**Features**:
- **Default System Prompt**: Custom instructions included in all new conversations
  - Stored via OpenSecret `createInstruction` with `is_default: true`
  - One default instruction per user
  - Empty + save removes the default instruction
  
- **Chat Appearance**:
  - Font family selection (Inter, system fonts, serif, monospace)
  - Text size: 13px - 19px (adjustable slider)
  - Live preview of changes
  - Applies to messages, reasoning, tool activity (code stays monospace)
  - Saved to localStorage

- **Text-to-Speech**:
  - Voice accent selection (Voxtral TTS voices)
  - Speech speed: 0.5x - 2.0x
  - Voice options organized by "Default voices" and "Reference accents"

**Implementation - System Prompt**:
```typescript
// Load existing
const response = await os.listInstructions({ limit: 100 });
const defaultInstruction = response.data.find((i) => i.is_default);

// Save new/update
if (instructionId) {
  if (prompt.trim() === "") {
    await os.deleteInstruction(instructionId);
  } else {
    await os.updateInstruction(instructionId, { prompt });
  }
} else if (prompt.trim() !== "") {
  await os.createInstruction({
    name: "User Preferences",
    prompt,
    is_default: true
  });
}
```

#### 2. Account (`/settings/account`)
User account information and management

#### 3. Security (`/settings/security`)  
Security-related settings

#### 4. Billing (`/settings/billing`)
Subscription and payment management

#### 5. API (`/settings/api`)
API keys and proxy configuration

#### 6. Team (`/settings/team`)
Team management (if applicable)

#### 7. History (`/settings/history`)
Conversation history management

#### 8. About (`/settings/about`)
App version and information

## Voice Features

### Voice Input (Recording)
**Location**: Microphone icon in chat composer  
**Function**: Record voice messages for transcription  
**Visual**: RecordingOverlay component during recording

**Features**:
- Real-time recording indicator
- Transcription to text before sending
- RecordRTC for audio capture

### Text-to-Speech (TTS)
**Location**: Settings → Preferences → Text-to-speech  
**Engine**: Voxtral TTS  
**Context**: `frontend/src/services/tts/TTSContext.tsx`

**Configuration**:
```typescript
// From ttsPreferences.ts
export const VOXTRAL_TTS_VOICE_OPTIONS = [
  // Default voices
  { value: "aura-asteria-en", label: "Asteria (American)", group: "Default voices" },
  { value: "aura-luna-en", label: "Luna (American)", group: "Default voices" },
  // ... more voices
  
  // Reference accents
  { value: "aura-angus-en", label: "Angus (Irish)", group: "Reference accents" },
  // ... more accents
];

export const TTS_MIN_PLAYBACK_SPEED = 0.5;
export const TTS_MAX_PLAYBACK_SPEED = 2.0;
export const TTS_PLAYBACK_SPEED_STEP = 0.1;
```

## Platform-Specific Features

### Desktop App (Tauri)
- Full document upload with PDF OCR (ONNX Runtime)
- Native file system dialogs
- Platform-specific keyboard shortcuts
- All features available

### Web Version
- Standard web browser capabilities
- Document upload shows platform dialog (directs to desktop)
- All image upload features available
- No OCR support

### Mobile (iOS/Android)
- Touch-optimized interface
- Responsive layouts (`useIsMobile`, `useIsLandscapeMobile`)
- Camera integration for image uploads
- Platform-specific navigation patterns

## Feature Availability Matrix

| Feature | Free Plan | Paid Plan | Platform Notes |
|---------|-----------|-----------|----------------|
| Web Search | ✓ Default ON | ✓ Default ON | All platforms |
| Image Upload | Limited | ✓ | Requires vision-capable model (Powerful) |
| Document Upload | Limited | ✓ | Full OCR on desktop only |
| Project Organization | ✓ | ✓ | All platforms |
| Quick Model | ✓ | ✓ | All platforms |
| Powerful Model | Limited | ✓ Default | Vision requires this model |
| Voice Input | ✓ | ✓ | All platforms with microphone |
| Text-to-Speech | ✓ | ✓ | All platforms |
| Custom System Prompt | ✓ | ✓ | All platforms |
| Chat Appearance | ✓ | ✓ | Local device settings |

## When NOT to Reference This Guide

**Do NOT use UI-specific language when**:
- User is accessing via OpenSecret API
- User mentions "proxy", "API", "CLI", "SDK"
- User is using third-party client
- Context indicates programmatic usage
- You're uncertain about interface

**Instead**:
- Focus on underlying capabilities
- Explain what's possible without UI instructions
- Use capability language ("web search can be enabled", "images can be analyzed")

## Keeping This Guide Current

This guide reflects Maple UX as of August 2024.

**To update**:
1. Check latest code in implementation reference files
2. Test actual UI behavior in development build
3. Update this guide with accurate information
4. Update `MAPLE_SYSTEM_INSTRUCTION.md` with key changes

## Implementation References

**Core UI Components**:
- `frontend/src/components/UnifiedChat.tsx` - Main chat interface
  - Lines ~5262-5283: Web search toggle
  - Lines ~5285-5349: File upload menu
  - Lines ~5256: Project picker integration
  
- `frontend/src/components/ConversationProjectPicker.tsx` - Project selection
- `frontend/src/components/ModelSelector.tsx` - Model dropdown
- `frontend/src/state/LocalStateContext.tsx` - State management, defaults
- `frontend/src/components/settings/PreferencesSettings.tsx` - Settings UI

**Settings Routes**:
- `frontend/src/routes/settings.*.tsx` - All settings pages

**Services**:
- `frontend/src/services/chatDraftSelection.ts` - Draft management
- `frontend/src/services/tts/TTSContext.tsx` - Text-to-speech
- `frontend/src/services/agentRuntimeService.ts` - Agent mode (desktop)

**Platform Detection**:
- `frontend/src/utils/platform.ts` - Platform utilities
