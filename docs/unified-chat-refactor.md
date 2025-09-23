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

## Conclusion

This refactor prioritizes simplicity and maintainability over premature optimization. By consolidating the chat interface into a single, well-organized component, we've created a solid foundation for the upcoming OpenAI Conversations/Responses API migration while maintaining all existing functionality.
