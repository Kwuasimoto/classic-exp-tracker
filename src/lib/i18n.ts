import { useSyncExternalStore } from "react";
import { en, type Messages } from "../locales/en";
import { es } from "../locales/es";
import { ja } from "../locales/ja";
import { ko } from "../locales/ko";
import { ptBR } from "../locales/pt-BR";
import { th } from "../locales/th";
import { vi } from "../locales/vi";
import { zhHans } from "../locales/zh-Hans";

/** The app's languages, each named in itself. */
export const LOCALES = [
  { id: "en", name: "English" },
  { id: "zh-Hans", name: "简体中文" },
  { id: "ko", name: "한국어" },
  { id: "ja", name: "日本語" },
  { id: "es", name: "Español" },
  { id: "pt-BR", name: "Português (Brasil)" },
  { id: "th", name: "ไทย" },
  { id: "vi", name: "Tiếng Việt" },
] as const;
export type Locale = (typeof LOCALES)[number]["id"];

const CATALOGS: Record<Locale, Messages> = { en, "zh-Hans": zhHans, ko, ja, es, "pt-BR": ptBR, th, vi };

/** The user's choice (a Locale), or absent to follow Windows. A display preference: it lives
 *  in this app's storage, shared by its windows (the popout follows the main window). */
const KEY = "locale";

const isLocale = (v: string | null): v is Locale => LOCALES.some((l) => l.id === v);

/** Windows' display language (WebView2 reports it first) if the app has it; English otherwise.
 *  Traditional Chinese gets English rather than Simplified: the user can pick 简体中文. */
export function systemLocale(): Locale {
  for (const tag of navigator.languages) {
    const t = tag.toLowerCase();
    if (t.startsWith("zh")) {
      if (/hant|-tw|-hk|-mo/.test(t)) continue;
      return "zh-Hans";
    }
    if (t.startsWith("ko")) return "ko";
    if (t.startsWith("ja")) return "ja";
    if (t.startsWith("es")) return "es";
    // Portuguese from Portugal reads Brazilian Portuguese fine; it's the closest the app has.
    if (t.startsWith("pt")) return "pt-BR";
    if (t.startsWith("th")) return "th";
    if (t.startsWith("vi")) return "vi";
    if (t.startsWith("en")) return "en";
  }
  return "en";
}

/** The saved choice, or null to follow Windows (also when storage is blocked). */
export function savedChoice(): Locale | null {
  try {
    const v = localStorage.getItem(KEY);
    return isLocale(v) ? v : null;
  } catch {
    return null;
  }
}

let current: Locale = savedChoice() ?? systemLocale();
const listeners = new Set<() => void>();

/** `lang` on the page: screen readers read in that language, and Windows picks the right
 *  Chinese, Japanese or Korean font for it (App.css). */
const apply = () => {
  document.documentElement.lang = current;
  listeners.forEach((l) => l());
};
apply();

export const currentLocale = (): Locale => current;
export const messages = (): Messages => CATALOGS[current];

/** Use `locale`, or follow Windows again (null). Saved for next time, if storage allows. */
export function chooseLocale(locale: Locale | null): void {
  try {
    if (locale === null) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, locale);
  } catch {
    // Storage blocked: the choice lasts for this run only.
  }
  current = locale ?? systemLocale();
  apply();
}

// Another window of the app changed the language.
window.addEventListener("storage", (e) => {
  if (e.key !== KEY) return;
  current = savedChoice() ?? systemLocale();
  apply();
});

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/** The current language's text; re-renders the component when the language changes. */
export function useMessages(): Messages {
  return CATALOGS[useSyncExternalStore(subscribe, currentLocale)];
}
