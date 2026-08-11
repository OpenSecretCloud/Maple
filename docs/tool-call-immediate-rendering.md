# Tool Call Immediate Rendering Refactor

> Historical refactor note. Event names, source locations, and observed gaps
> may no longer match the current implementation. Use current source,
> `AGENTS.md`, and the applicable `.agents/skills/` workflow as the authority.

## Problem Statement

Tool calls don't render immediately during streaming in UnifiedChat. Currently:

1. **Tool call arrives** (`tool_call.created` event) → stored in Map, not displayed
2. **Tool output arrives** (`tool_output.created` event) → stored in Map, not displayed  
3. **Assistant message created** (`response.output_item.added` type="message") → empty message created
4. **Text starts streaming** (`response.output_text.delta`) → **NOW** tool calls finally render

**User experience**: 5+ second delay between tool call and seeing any visual feedback.

## Streaming Event Sequence (from logs)

```
🔵 response.created
🔵 response.in_progress
🔵 tool_call.created          ← Call created, but not displayed yet
    hasAssistantId: false
    toolCallsCount: 0
    
🔵 tool_output.created         ← Output ready, but not displayed yet
    hasAssistantId: false
    toolCallsCount: 1
    
🔵 response.output_item.added  ← NOW assistant message created
    type: "message"
    hasAssistantId: true
    
🔵 response.output_text.delta  ← NOW tool calls finally display!
```

## Root Cause

The architecture tightly couples "displayed items" with the API's `output_item` concept:

1. **Current architecture**: Wait for `output_item.added` (type="message") to create container
2. **Tool calls arrive first**: Before the message container exists
3. **Buffered in Map**: Waiting to be grouped inside assistant message
4. **Finally rendered**: Only when text deltas start arriving

## Solution Approach

### Original Architecture
```
Message (assistant) {
  content: [tool_call, tool_output, text]
}
```
- Everything nested inside message's content array
- Tool calls/outputs don't exist as standalone items
- Must wait for message to exist before rendering

### New Architecture (Flat List)
```
messages = [
  { type: "message", role: "user", ... },
  { type: "function_call", call_id: "...", ... },
  { type: "function_call_output", call_id: "...", ... },
  { type: "message", role: "assistant", ... }
]
```
- Each item is independent at top level
- Render immediately as events arrive
- Visual pairing handled by renderer (matching `call_id`)
- Matches how LLMs and the API actually model conversations

## Implementation Progress

### ✅ Completed

1. **Simplified type system**
   - Changed `type Message = ConversationItem` to use OpenAI's native types
   - Removed custom `Message` interface with nested content
   - Removed `timestamp` field (wasn't used for anything)

2. **Simplified data conversion**
   - `convertItemsToMessages` now just casts items as-is
   - No more grouping/buffering logic (126 lines → 5 lines)

### 🚧 In Progress

3. **Update MessageList rendering** (partially complete)
   - Need to handle different item types: `message`, `function_call`, `function_call_output`, `web_search_call`
   - Render each type independently
   - Keep visual pairing logic (tools render together when adjacent)
   - Update to check `item.type` instead of `message.role`

### ❌ TODO

4. **Update streaming logic** (processStreamingResponse)
   - Create items immediately as events arrive (no Map buffering)
   - `tool_call.created` → `setMessages([...prev, toolCallItem])`
   - `tool_output.created` → `setMessages([...prev, toolOutputItem])`
   - `response.output_text.delta` → update assistant message text
   - Remove the `toolCalls` Map entirely

5. **Update user message creation**
   - Change from custom Message type to OpenAI's Message format
   - Match the `type: "message"` structure

6. **Fix TypeScript errors**
   - Update all code that checks `message.role` to check `item.type` 
   - Update all code that assumes `message.content` exists on all items
   - Handle different item types in scroll logic, status checks, etc.

## Benefits of Flat Structure

1. **Immediate rendering**: Items render as soon as they arrive
2. **Matches API model**: No impedance mismatch with OpenAI's data structure
3. **Simpler streaming logic**: No buffering, no grouping, just add items
4. **Matches LLM mental model**: Tools are distinct operations, not nested content
5. **Easier to maintain**: Less custom mapping logic

## Alternative: Minimal Fix

If full refactor is too large, we could do a smaller fix:

1. Keep current grouped structure
2. Create assistant message **eagerly** (on first tool call, not on output_item.added)
3. Add items to message immediately (no Map buffering)

This would fix the immediate issue but keep the coupling between display and API structure.

## Testing Plan

1. **Streaming test**: Tool call should appear immediately (< 100ms after event)
2. **Pairing test**: When output arrives, should render as grouped pair
3. **Orphan test**: Tool output without call should render standalone
4. **Loading test**: Old conversations load correctly with tool calls
5. **Scroll test**: Auto-scroll behavior still works with flat structure

## Files Changed

- `/frontend/src/components/UnifiedChat.tsx` - Main refactor
  - Type definitions (lines 75-102)
  - convertItemsToMessages (lines 114-120)
  - MessageList rendering (lines 492+)
  - processStreamingResponse (lines 1724+)
  - handleSendMessage user message creation (lines 1926+)

## Current State

The code is **partially refactored** but **not working**:
- Type system simplified ✅
- Data conversion simplified ✅
- Rendering needs update for flat items ❌
- Streaming needs update to create items immediately ❌
- TypeScript errors need fixing ❌

**Decision needed**: Complete the flat refactor OR revert and do minimal fix?
