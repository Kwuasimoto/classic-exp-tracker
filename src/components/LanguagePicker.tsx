import { useEffect, useRef, useState } from "react";
import { chooseLocale, LOCALES, savedChoice, systemLocale, useMessages, type Locale } from "../lib/i18n";

const nameOf = (id: Locale) => LOCALES.find((l) => l.id === id)?.name ?? id;

/** The rail's language button and its menu: Windows' language (the default), or one of the
 *  app's languages, each named in itself. Closes on a choice, a click outside or Escape. */
export function LanguagePicker() {
  const m = useMessages();
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState<Locale | null>(savedChoice);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === "Escape" : !box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", close);
    };
  }, [open]);

  const choose = (locale: Locale | null) => {
    chooseLocale(locale);
    setChoice(locale);
    setOpen(false);
  };

  const options: Array<{ id: Locale | null; label: string }> = [
    { id: null, label: m.language.windows(nameOf(systemLocale())) },
    ...LOCALES.map((l) => ({ id: l.id, label: l.name })),
  ];
  return (
    <div className="language" ref={box}>
      <button className={open ? "rail-btn open" : "rail-btn"} aria-label={m.language.button} title={m.language.title} aria-expanded={open} onClick={() => setOpen(!open)}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="9" />
          <path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
        </svg>
      </button>
      {open && (
        <div className="flyout" role="dialog" aria-label={m.language.title}>
          {options.map((o) => (
            <button key={o.id ?? "windows"} className={o.id === choice ? "flyout-option current" : "flyout-option"} aria-pressed={o.id === choice} lang={o.id ?? undefined} onClick={() => choose(o.id)}>
              {o.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
