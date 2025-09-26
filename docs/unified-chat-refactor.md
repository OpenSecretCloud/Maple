# Unified Chat Refactor - Phase 1

## Overview

This document describes the initial refactor of Maple's chat interface in preparation for migrating from the current localStorage-based chat system to OpenAI's Conversations/Responses API.

## Motivation

The existing chat architecture had several pain points:

1. **Scattered State Management**: Chat state was distributed across multiple components and routes:
   - `frontend/src/routes/index.tsx` - Home page with ChatBox
   - `frontend/src/routes/_auth.chat.$chatId.tsx` - Individual chat route
   - `frontend/src/components/ChatBox.tsx` - Shared chat input component
   - Complex prop drilling and state synchronization between these components

2. **Complex Routing Logic**: The system required careful coordination between routes, with state being passed through navigation params, leading to:
   - Difficult debugging when state got out of sync
   - Re-rendering and remounting issues on navigation
   - Complex URL management logic

3. **Preparation for API Migration**: The upcoming switch to OpenAI's Conversations/Responses API requires a simpler architecture that can handle:
   - Server-side conversation state
   - Streaming responses
   - No dependency on localStorage for chat history

## Architectural Decisions

### 1. Monolithic Component Design

We created a single `UnifiedChat` component that contains all chat functionality:

```typescript
// frontend/src/components/UnifiedChat.tsx
export function UnifiedChat() {
  // ALL chat state lives here
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [isGenerating, setIsGenerating] = useState(false);
  // ...
}
```

**Rationale**:
- Following the principle "Premature abstraction is the root of all evil"
- Colocated code is easier to debug and understand
- No state synchronization bugs between components
- Similar to how large tech companies (Meta, etc.) handle complex components

### 2. URL Management Without Navigation

Instead of using TanStack Router navigation (which causes remounting), we use browser-native `window.history.replaceState()`:

```javascript
// Update URL without any navigation/reload
const usp = new URLSearchParams(window.location.search);
usp.set("conversation_id", newChatId);
window.history.replaceState(null, "", `/?${usp.toString()}`);
```

**Benefits**:
- No component remounting
- No state loss
- URL updates for shareability/bookmarking
- No "route not found" errors (query params don't need routes)

### 3. Query Parameters Over Route Parameters

We use `?conversation_id=xxx` instead of `/chat/xxx`:

- **Before**: `/chat/123` - Requires route file, causes navigation
- **After**: `/?conversation_id=123` - No route needed, just URL update

This approach avoids the need for route configuration while maintaining URL-based state.

### 4. Preserved Existing Infrastructure

We maintained backward compatibility:
- Old `/chat/$chatId` routes still work
- Existing Sidebar component is reused
- Auth logic and modals (team setup, API keys) remain functional
- Search parameters for callbacks (`team_setup`, `credits_success`) preserved

## Implementation Details

### File Structure

**New Files**:
- `frontend/src/components/UnifiedChat.tsx` - The unified chat component
- `frontend/src/routes/index.backup.tsx` - Backup of original index

**Modified Files**:
- `frontend/src/routes/index.tsx` - Simplified to show Marketing or UnifiedChat based on auth
- `frontend/src/components/Sidebar.tsx` - Updated "New Chat" to clear conversation_id

### State Management

Currently using local React state with mocked responses:

```typescript
// Mock AI response - will be replaced with OpenAI conversations API
setTimeout(() => {
  const assistantMessage: Message = {
    id: `msg-${Date.now()}-ai`,
    role: "assistant",
    content: "Hello world! This is a mocked response...",
    timestamp: Date.now()
  };
  setMessages(prev => [...prev, assistantMessage]);
}, 1000);
```

This will be replaced with actual API calls in Phase 2.

### New Chat Flow

1. User clicks "New Chat" in sidebar
2. Sidebar clears `conversation_id` from URL
3. Dispatches 'newchat' event
4. UnifiedChat listens and clears messages
5. Input field gets focus

## Benefits Achieved

1. **Simplified Codebase**: ~250 lines in one file vs ~500+ lines across multiple files
2. **No State Synchronization Issues**: Single source of truth
3. **Better Performance**: No unnecessary re-renders or navigation
4. **Easier Debugging**: All logic in one place
5. **Ready for API Migration**: Clean foundation for OpenAI integration

## Next Steps (Phase 2)

1. **OpenAI Conversations API Integration**:
   - Replace mock responses with actual API calls
   - Implement streaming responses
   - Handle conversation creation and management

2. **Remove localStorage Dependency**:
   - Migrate chat history to server-side storage
   - Update Sidebar to fetch from API instead of localStorage

3. **Error Handling & Edge Cases**:
   - Handle API failures gracefully
   - Implement retry logic
   - Add loading states for conversation fetching

## Design Philosophy

This refactor follows the principle of **"Make it work, make it right, make it fast"**:

1. **Make it work**: Single component with all functionality (current state)
2. **Make it right**: Will be achieved with API integration
3. **Make it fast**: Can optimize/split components later if needed

By avoiding premature optimization and keeping everything in one place, we've created a maintainable foundation that can evolve as requirements become clearer.

## Technical Decisions Explained

### Why Not Cache Conversations?

We explicitly decided against caching for now:
- Most users work on one conversation at a time
- API is fast enough that loading isn't painful
- Adds complexity that may not be needed
- Can be added later if users report performance issues

### Why Query Parameters?

- No route configuration needed
- Works immediately without router setup
- Prevents "route not found" errors
- Can be migrated to proper routes later if needed

### Why Keep Everything in One Component?

- Based on real-world experience at major tech companies
- Easier to understand and debug
- No props drilling or state synchronization
- Can be split later when natural boundaries emerge

## Current Implementation Status

### ✅ Features Successfully Implemented

The UnifiedChat component now includes these fully working features:

#### Core Chat Functionality
- **Conversations/Responses API Integration** - Full server-side state management with OpenAI-compatible endpoints
- **Streaming responses** - Real-time SSE event handling for all response types
- **Message deduplication** - Smart ID management using server-assigned IDs with smooth local-to-server transitions
- **URL-based conversation routing** - Query parameter approach (`?conversation_id=xxx`) avoiding route configuration
- **5-second polling** - Automatic synchronization for cross-device conversations
- **Conversation lifecycle** - Lazy creation, loading from URL, switching between conversations

#### User Interface
- **Modern ChatGPT-style UI** - Clean aesthetics with hover states and subtle backgrounds
- **Auto-scrolling** - Intelligent scroll on new messages (user and assistant)
- **Copy to clipboard** - One-click copy for assistant messages
- **React.memo optimization** - MessageList component prevents re-renders during input
- **Responsive sidebar** - Mobile-friendly with toggle button
- **Centered input for new chats** - Beautiful welcome screen with logo and prompt
- **Fixed input for active chats** - Standard chat interface when conversation is active
- **Mobile new chat button** - Quick access button in mobile header when in a conversation
- **Consistent mobile UI** - Aligned headers and consistent button styling across sidebar and main chat

#### Multimodal Support
- **Image attachments** - Support for JPEG, PNG, WebP up to 10MB
- **Document parsing** - PDF, TXT, MD support (PDF requires Tauri)
  - Fixed Tauri command: Uses `extract_document_content` instead of `parse_document`
  - Simplified JSON format: Documents stored as `{ document: { filename, text_content } }`
  - Removed unnecessary `status` and `errors` fields from document structure
  - Proper markdown rendering with document preview button
- **Attachment preview** - Visual previews with remove capability
- **Auto model switching** - Automatically selects vision-capable models when images added
- **Plus button dropdown** - Clean attachment interface
- **Proper OpenAI format** - Uses `input_text`, `input_image`, `output_text` content types

#### Billing & Access Control
- **Tier-based features** - Starter (images), Pro/Team (documents)
- **Upgrade prompts** - Contextual dialogs when accessing restricted features
- **Model selector integration** - Shows available models based on user's plan

#### Error Handling
- **404 recovery** - Gracefully handles non-existent conversations
- **Network error display** - User-friendly error messages
- **Silent polling failures** - Doesn't interrupt user experience
- **Attachment validation** - File type and size checks with clear feedback

### ✅ Recently Implemented Features

#### Voice Recording (Completed December 2024)
- **Voice recording** - Microphone input with RecordRTC
- **Whisper transcription** - Convert speech to text via OpenSecret API
- **Recording overlay** - Visual feedback with waveform animation
- **Proper overlay positioning** - Covers only input area, not full page
- **Access control** - Requires Pro/Team tier and Whisper model availability
- **Error handling** - Clear messages for permission issues

### ❌ Features Not Yet Migrated

These features exist in the old components but haven't been implemented in UnifiedChat:

#### TTS Features (Postponed - API not working)
- **Text-to-Speech (TTS)** - Kokoro voice synthesis with play/stop controls
- **Auto-play TTS** - Automatic playback for voice-initiated messages
- **Audio manager** - Prevents multiple TTS playing simultaneously

#### Scroll Behavior Improvements Needed
- **Scroll-to-bottom button** - Floating button when scrolled up in conversation
- **Better auto-scroll logic** - Need to match old behavior:
  - Auto-scroll when user sends a message
  - Auto-scroll when assistant starts streaming
  - Maintain scroll position when not at bottom
  - Smooth scrolling animations

#### System Prompt (Coming Soon via API)
- **System prompt support** - Will be handled via new API, not frontend input
- **Collapsible display** - Will need UI for showing system prompts when implemented

#### UI/UX Features
- **Draft message persistence** - localStorage backup of unsent messages

#### Advanced Features
- **Document metadata tracking** - Preserve filename and full content
- **Multi-file selection** - Batch image uploads
- **Message-specific actions** - Per-message TTS controls

### 🎯 Feature Prioritization

Based on user value and implementation complexity:

#### High Priority (Essential)
1. ✅ **Voice Input** - COMPLETED! Recording and transcription working
2. ✅ **Token Management** - HANDLED BY BACKEND! Intelligent compression on server-side
3. ✅ **Streaming indicators** - COMPLETED! Different implementation but working well
4. **Scroll-to-bottom button** - Simple but important UX improvement (needs implementation)
5. **TTS** - Postponed until API is fixed

#### Medium Priority (Nice to Have)
6. **System prompt support** - Coming via new API
7. **Draft persistence** - Prevents data loss on refresh

#### Low Priority (Already Done or Not Needed)
9. ✅ **Mobile new chat button** - Already implemented
10. ✅ **Token warnings** - Not needed, backend handles compression automatically
11. **Message-specific TTS controls** - Will implement when TTS API is fixed

### 🏗️ Architecture Improvements Achieved

The refactor has delivered significant architectural improvements:

1. **Single Component Architecture** - All logic in UnifiedChat.tsx, no prop drilling
2. **Server-Driven State** - No localStorage dependencies for chat data
3. **Clean URL Management** - Query parameters avoid complex routing
4. **Optimized Rendering** - Strategic use of React.memo prevents unnecessary re-renders
5. **Proper Error Boundaries** - Graceful handling of API failures
6. **Event-Based Communication** - Clean integration with sidebar via custom events
7. **Abort Controllers** - Proper cleanup of in-flight requests
8. **Resource Management** - Proper cleanup of object URLs and event listeners

### 📊 Comparison with Old Architecture

| Aspect | Old Implementation | New UnifiedChat |
|--------|-------------------|----------------|
| **Files** | 3+ components, multiple routes | Single component |
| **State Management** | Props, localStorage, context | Local React state + API |
| **Chat Persistence** | localStorage | Server-side via API |
| **Routing** | `/chat/:chatId` with route files | `?conversation_id=xxx` query params |
| **Message IDs** | Client-generated only | Server-assigned with local fallback |
| **Polling** | None | 5-second interval with cursor |
| **Code Complexity** | ~500+ lines across files | ~1276 lines in one file |
| **Debugging** | Difficult (scattered logic) | Easy (colocated code) |

### 🚀 Next Steps

1. **Implement Voice Features** - Add recording and TTS for accessibility
2. **Add Token Management** - Implement counting and compression
3. **Enhance UX** - Add scroll-to-bottom and streaming indicators
4. **Performance Optimization** - Consider splitting component if it grows much larger
5. **Testing** - Add comprehensive tests for the unified component

## Conclusion

The UnifiedChat refactor has successfully achieved its primary goals:
- ✅ Simplified architecture with single component
- ✅ Full Conversations/Responses API integration
- ✅ Removed localStorage dependencies for chat data
- ✅ Maintained all essential functionality
- ✅ Improved performance with React.memo
- ✅ Created foundation for future enhancements

While some features from the old implementation haven't been migrated yet, the core chat experience is fully functional and the architecture is much cleaner. The missing features are primarily UX enhancements that can be added incrementally based on user feedback and priorities.
