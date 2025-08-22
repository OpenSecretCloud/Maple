import i18next, { InitOptions } from 'i18next';
import { initReactI18next } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';

/**
 * Resolve the locale to use:
 *   1. Try the native Tauri localization plugin.
 *   2. Fallback to the browser language.
 */
async function resolveLocale(): Promise<string> {
  try {
    const native = await invoke<string>('plugin:localization|get_locale');
    if (native) {
      console.log('[i18n] Native locale detected:', native);
      return native;
    }
  } catch (e) {
    console.warn('[i18n] Native locale unavailable, using navigator.language', e);
  }
  
  const browserLocale = navigator.language || 'en-US';
  console.log('[i18n] Using browser locale:', browserLocale);
  return browserLocale;
}

/**
 * Load the JSON file that matches the locale.
 * Vite's import.meta.glob creates a map of all json files in the locales folder.
 * The path needs to be relative to the project root where public/ is located.
 */
const localeModules = import.meta.glob('/public/locales/*.json') as Record<
  string,
  () => Promise<{ default: unknown }>
>;

async function loadResources(requested: string) {
  const short = requested.split('-')[0]; // en-US → en
  const path = `/public/locales/${short}.json`;

  console.log('[i18n] Looking for locale file:', path);
  console.log('[i18n] Available locale modules:', Object.keys(localeModules));

  const loader = localeModules[path];
  if (!loader) {
    console.warn(`[i18n] Locale ${short} not found – falling back to English`);
    const englishLoader = localeModules['/public/locales/en.json'];
    if (!englishLoader) {
      console.error('[i18n] English fallback not found!');
      return { en: { translation: {} } };
    }
    const mod = await englishLoader();
    return { en: { translation: mod.default as Record<string, unknown> } };
  }

  const mod = await loader();
  return { [short]: { translation: mod.default as Record<string, unknown> } };
}

/**
 * Initialize i18next – call once at startup.
 */
export async function initI18n(): Promise<void> {
  console.log('[i18n] Initializing i18next...');
  
  const locale = await resolveLocale();
  const resources = await loadResources(locale);
  const short = locale.split('-')[0]; // en-US → en

  console.log('[i18n] Using locale:', short);
  console.log('[i18n] Resources loaded:', Object.keys(resources));

  const options: InitOptions = {
    lng: short,
    fallbackLng: 'en',
    resources,
    interpolation: { escapeValue: false },
    react: { useSuspense: false },
    debug: import.meta.env.DEV // Enable debug logging in development
  };

  await i18next.use(initReactI18next).init(options);
  console.log('[i18n] i18next initialized successfully');
}
