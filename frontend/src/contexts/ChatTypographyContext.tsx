import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode
} from "react";

import {
  applyChatTypography,
  DEFAULT_CHAT_FONT_FAMILY,
  DEFAULT_CHAT_FONT_SIZE,
  getStoredChatFontFamily,
  getStoredChatFontSize,
  rememberChatFontFamily,
  rememberChatFontSize,
  resetChatTypographyPreferences,
  type ChatFontFamily,
  type ChatTypographyStorage,
  type ChatTypographyStyle
} from "@/services/chatTypographyPreferences";

interface ChatTypographyContextValue {
  fontFamily: ChatFontFamily;
  fontSize: number;
  setFontFamily: (fontFamily: ChatFontFamily) => void;
  setFontSize: (fontSize: number) => void;
  resetTypography: () => void;
  hasCustomTypography: boolean;
}

interface ChatTypographyProviderProps {
  children: ReactNode;
  storage?: ChatTypographyStorage | null;
  rootStyle?: ChatTypographyStyle | null;
}

const ChatTypographyContext = createContext<ChatTypographyContextValue | null>(null);

export function ChatTypographyProvider({
  children,
  storage,
  rootStyle
}: ChatTypographyProviderProps) {
  const [fontFamily, setFontFamilyState] = useState(() => getStoredChatFontFamily(storage));
  const [fontSize, setFontSizeState] = useState(() => getStoredChatFontSize(storage));
  const fontFamilyRef = useRef(fontFamily);
  const fontSizeRef = useRef(fontSize);

  useEffect(() => {
    applyChatTypography({ fontFamily, fontSize }, rootStyle);
  }, [fontFamily, fontSize, rootStyle]);

  const setFontFamily = useCallback(
    (nextFontFamily: ChatFontFamily) => {
      const safeFontFamily = rememberChatFontFamily(nextFontFamily, storage);
      fontFamilyRef.current = safeFontFamily;
      applyChatTypography({ fontFamily: safeFontFamily, fontSize: fontSizeRef.current }, rootStyle);
      setFontFamilyState(safeFontFamily);
    },
    [rootStyle, storage]
  );

  const setFontSize = useCallback(
    (nextFontSize: number) => {
      const safeFontSize = rememberChatFontSize(nextFontSize, storage);
      fontSizeRef.current = safeFontSize;
      applyChatTypography({ fontFamily: fontFamilyRef.current, fontSize: safeFontSize }, rootStyle);
      setFontSizeState(safeFontSize);
    },
    [rootStyle, storage]
  );

  const resetTypography = useCallback(() => {
    resetChatTypographyPreferences(storage);
    fontFamilyRef.current = DEFAULT_CHAT_FONT_FAMILY;
    fontSizeRef.current = DEFAULT_CHAT_FONT_SIZE;
    applyChatTypography(
      { fontFamily: DEFAULT_CHAT_FONT_FAMILY, fontSize: DEFAULT_CHAT_FONT_SIZE },
      rootStyle
    );
    setFontFamilyState(DEFAULT_CHAT_FONT_FAMILY);
    setFontSizeState(DEFAULT_CHAT_FONT_SIZE);
  }, [rootStyle, storage]);

  const value = useMemo(
    () => ({
      fontFamily,
      fontSize,
      setFontFamily,
      setFontSize,
      resetTypography,
      hasCustomTypography:
        fontFamily !== DEFAULT_CHAT_FONT_FAMILY || fontSize !== DEFAULT_CHAT_FONT_SIZE
    }),
    [fontFamily, fontSize, resetTypography, setFontFamily, setFontSize]
  );

  return <ChatTypographyContext.Provider value={value}>{children}</ChatTypographyContext.Provider>;
}

export function useChatTypography(): ChatTypographyContextValue {
  const context = useContext(ChatTypographyContext);
  if (!context) {
    throw new Error("useChatTypography must be used within a ChatTypographyProvider");
  }
  return context;
}
