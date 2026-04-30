/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Dev-only: number of fake sidebar chats, or "0"/"false" to disable. */
  readonly VITE_MOCK_SIDEBAR_CHAT_COUNT?: string;
}
