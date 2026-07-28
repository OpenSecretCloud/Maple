import {
  composerHasRetainedDraft,
  createChatComposerState,
  type ChatComposerState
} from "@/contexts/ChatRuntimeContext";
import {
  ChatRuntimeStore,
  createChatDraftKey,
  type ChatRuntimeKey,
  type DraftChatRuntimeKey
} from "@/services/chatRuntimeStore";
import { isDraftChatRuntimeKey } from "@/services/chatRuntimeNavigation";

export function resumeOrCreateChatDraftKey<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  draftProjectId: string | null,
  createDraftKey: () => DraftChatRuntimeKey = createChatDraftKey
): DraftChatRuntimeKey {
  const rememberedKey = store.getRememberedDraftKey(draftProjectId);
  if (rememberedKey) {
    const canonicalKey = store.resolveKey(rememberedKey);
    const snapshot = store.get(rememberedKey);
    const activeKey = store.getActiveKey();
    const isCurrentVisibleDraft =
      activeKey !== null &&
      store.resolveKey(activeKey) === canonicalKey &&
      store.isChatVisible(rememberedKey);
    const canResume =
      canonicalKey === rememberedKey &&
      !isCurrentVisibleDraft &&
      (snapshot === undefined ||
        (snapshot.conversation === null &&
          snapshot.messages.length === 0 &&
          !snapshot.isGenerating &&
          !snapshot.assistantStreaming &&
          snapshot.runToken === null &&
          snapshot.composer.draftProjectId === draftProjectId &&
          composerHasRetainedDraft(snapshot.key, snapshot.composer)));

    if (canResume) return rememberedKey;
  }

  return createAndRememberChatDraftKey(store, draftProjectId, createDraftKey);
}

export function rootChatDraftKeyAfterProjectDeletion<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  createDraftKey: () => DraftChatRuntimeKey = createChatDraftKey
): DraftChatRuntimeKey {
  return resumeOrCreateChatDraftKey(store, null, createDraftKey);
}

export function draftScopeForRuntimeSelection<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  key: ChatRuntimeKey,
  fallbackScopeId: string | null
): string | null {
  const snapshot = store.get(key);
  return snapshot ? snapshot.composer.draftProjectId : fallbackScopeId;
}

export function rememberChatDraftInScope<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  key: ChatRuntimeKey,
  scopeId: string | null
): boolean {
  const canonicalKey = store.resolveKey(key);
  if (!isDraftChatRuntimeKey(canonicalKey)) return false;
  store.rememberDraftKey(scopeId, canonicalKey);
  store.updateActivityGroup(canonicalKey, scopeId);
  return true;
}

export function createAndRememberChatDraftKey<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  scopeId: string | null,
  createDraftKey: () => DraftChatRuntimeKey = createChatDraftKey
): DraftChatRuntimeKey {
  const freshKey = createDraftKey();
  store.ensure(freshKey, { composer: createChatComposerState(scopeId) });
  rememberChatDraftInScope(store, freshKey, scopeId);
  return freshKey;
}

export function moveRememberedChatDraftToScope<TConversation, TMessage>(
  store: ChatRuntimeStore<TConversation, TMessage, ChatComposerState>,
  key: ChatRuntimeKey,
  nextDraftProjectId: string | null
): boolean {
  const canonicalKey = store.resolveKey(key);
  if (!isDraftChatRuntimeKey(canonicalKey)) return false;

  const snapshot = store.get(canonicalKey);
  if (!snapshot || snapshot.conversation !== null) return false;

  const previousDraftProjectId = snapshot.composer.draftProjectId;
  if (previousDraftProjectId !== nextDraftProjectId) {
    store.update(canonicalKey, (current) => ({
      ...current,
      composer: {
        ...current.composer,
        draftProjectId: nextDraftProjectId
      }
    }));
  }

  store.forgetRememberedDraftKey(previousDraftProjectId, canonicalKey);
  rememberChatDraftInScope(store, canonicalKey, nextDraftProjectId);
  return true;
}
