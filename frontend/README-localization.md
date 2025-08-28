# Localization (i18n) Guide for Maple

This document explains how the automatic UI localization system works in Maple and how to add new languages.

## How It Works

Maple automatically detects the user's operating system language and displays the UI in that language:

1. **Locale Detection**: Uses `tauri-plugin-localization` to get the native OS locale
2. **Fallback**: Falls back to browser language (`navigator.language`) if native detection fails
3. **Translation Loading**: Dynamically loads the appropriate JSON translation file
4. **UI Rendering**: React components use `useTranslation()` hook to display localized strings

## Current Supported Languages

- **English** (`en`) - Default and fallback language
- **French** (`fr`) - Complete translations 
- **Spanish** (`es`) - Complete translations

## File Structure

```
frontend/
├── public/locales/          # Translation files
│   ├── en.json             # English (default)
│   ├── fr.json             # French
│   └── es.json             # Spanish
├── src/
│   ├── utils/i18n.ts       # i18n configuration
│   ├── main.tsx            # i18n initialization
│   └── components/         # Components using translations
└── src-tauri/
    ├── Cargo.toml          # Rust dependencies
    ├── src/lib.rs          # Plugin registration
    ├── tauri.conf.json     # Asset protocol config
    └── gen/apple/maple_iOS/Info.plist  # iOS language declarations
```

## Adding a New Language

### 1. Create Translation File

1. Copy `public/locales/en.json` to `public/locales/{code}.json` (e.g., `de.json` for German)
2. Translate all the strings while keeping the same key structure:

```json
{
  "app": {
    "title": "Maple - Private KI-Chat",
    "welcome": "Willkommen bei Maple",
    "description": "Private KI-Chat mit vertraulicher Datenverarbeitung"
  },
  "auth": {
    "signIn": "Anmelden",
    "signOut": "Abmelden",
    "email": "E-Mail",
    "password": "Passwort"
  }
  // ... continue with all keys
}
```

### 2. Update iOS Configuration

Edit `src-tauri/gen/apple/maple_iOS/Info.plist` and add your language code:

```xml
<key>CFBundleLocalizations</key>
<array>
    <string>en</string>
    <string>fr</string>
    <string>es</string>
    <string>de</string>  <!-- Add your new language -->
</array>
```

### 3. Test the Implementation

1. **Development**: `bun tauri dev`
   - Change your OS language settings
   - Restart the app to see the new language

2. **iOS**: `bun tauri build --target ios`
   - Build and run in iOS Simulator
   - Change device language in Settings app
   - Test the localized UI

## Using Translations in Components

### Basic Usage

```tsx
import { useTranslation } from 'react-i18next';

function MyComponent() {
  const { t } = useTranslation();

  return (
    <div>
      <h1>{t('app.title')}</h1>
      <button>{t('button.save')}</button>
    </div>
  );
}
```

### With Variables

```tsx
// Translation with interpolation
const message = t('auth.welcome', { name: 'John' });

// In en.json:
// "auth": { "welcome": "Welcome, {{name}}!" }
```

### Language Switching (Optional)

```tsx
const { i18n } = useTranslation();

// Manually change language (for testing/admin purposes)
i18n.changeLanguage('fr');
```

## Technical Details

### Dependencies

- **Frontend**: `i18next`, `react-i18next`
- **Backend**: `tauri-plugin-localization`

### Initialization Flow

1. `main.tsx` calls `initI18n()` before rendering
2. `i18n.ts` resolves the locale using Tauri plugin
3. Appropriate JSON file is loaded dynamically
4. i18next is initialized with the translations
5. React app renders with localized strings

### Fallback Strategy

1. Try native OS locale (e.g., `en-US`)
2. Extract language code (`en-US` → `en`)
3. Load matching JSON file (`en.json`)
4. If not found, fall back to English
5. If English fails, use empty translations

## Platform Support

| Platform | Locale Detection | Status |
|----------|------------------|--------|
| **Desktop** (Windows/macOS/Linux) | ✅ Native OS locale | Fully supported |
| **iOS** | ✅ Device language | Fully supported |
| **Web** | ✅ Browser language | Fallback only |

## Troubleshooting

### Language Not Changing

1. Check that the JSON file exists in `public/locales/`
2. Verify iOS `Info.plist` includes the language code
3. Restart the app after changing OS language
4. Check browser console for i18n loading errors

### Missing Translations

1. Compare your JSON structure with `en.json`
2. Ensure all keys match exactly (case-sensitive)
3. Check for syntax errors in JSON files
4. Use the `t()` function's fallback: `t('key', { defaultValue: 'fallback' })`

### iOS Build Issues

1. Ensure Xcode project is regenerated: `bun tauri build --target ios`
2. Check that `CFBundleLocalizations` is properly formatted
3. Clean build folder if needed

## Performance Notes

- Translation files are loaded asynchronously on startup
- Only the detected language file is loaded (not all languages)
- Vite's `import.meta.glob` ensures efficient bundling
- First render waits for i18n initialization to prevent FOUC

## Future Enhancements

- [ ] Add more languages (German, Italian, Portuguese, Japanese, etc.)
- [ ] Implement plural forms for complex languages
- [ ] Add context-aware translations
- [ ] Create translation management workflow
- [ ] Add RTL language support (Arabic, Hebrew)

---

For questions or issues with localization, please check the main README or open an issue on GitHub.
