import { useState } from "react";
import { useMessages } from "../lib/i18n";
import { removeTranslatorKey, saveTranslatorKey, type AppError } from "../lib/ipc";

/** Where a Gemini API key is made. */
const KEY_PAGE = "aistudio.google.com/apikey";

/** The Gemini API key the translator uses (gemini.rs): the user's own, kept in Windows
 *  Credential Manager. Saving checks it with Google first; the page never sees it again. */
export function TranslatorKey({ saved, onError, onChange }: { saved: boolean | null; onError: (e: AppError | null) => void; onChange: () => Promise<void> }) {
  const m = useMessages();
  const t = m.translator;
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState(false);
  // The model that answered when the key was saved here.
  const [answered, setAnswered] = useState<string | null>(null);
  // Removing asks first.
  const [removing, setRemoving] = useState(false);

  const save = async () => {
    setChecking(true);
    const res = await saveTranslatorKey(key);
    setChecking(false);
    onError(res.success ? null : res.error);
    if (res.success) {
      setKey("");
      setAnswered(res.data);
    }
    await onChange();
  };

  const remove = async () => {
    const res = await removeTranslatorKey();
    onError(res.success ? null : res.error);
    setRemoving(false);
    setAnswered(null);
    await onChange();
  };

  return (
    <section className="panel">
      <h2>{t.keyTitle}</h2>
      <p className="translator-note">{t.keyIntro}</p>
      {saved === null ? (
        <p className="caption">{t.checking}</p>
      ) : saved ? (
        <div className="key-row">
          <span className="ok-text key-state">{answered !== null ? t.keyWorks(answered) : t.keySaved}</span>
          {removing ? (
            <>
              <button className="btn small danger" onClick={() => void remove()}>{t.removeKey}</button>
              <button className="btn small" onClick={() => setRemoving(false)}>{m.common.keep}</button>
            </>
          ) : (
            <button className="btn small" onClick={() => setRemoving(true)}>{t.removeKey}</button>
          )}
        </div>
      ) : (
        <>
          <p className="translator-note">
            {t.keyHow[0]}<code>{KEY_PAGE}</code>{t.keyHow[1]}
          </p>
          <form
            className="key-row"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <input
              className="key-input"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={key}
              placeholder={t.keyPlaceholder}
              aria-label={t.keyLabel}
              disabled={checking}
              onChange={(e) => setKey(e.target.value)}
            />
            <button type="submit" className="btn primary" disabled={checking || key.trim() === ""}>{t.saveKey}</button>
            {checking && <span className="caption" role="status">{t.checkingKey}</span>}
          </form>
        </>
      )}
      <p className="caption translator-note">{t.keyPrivacy}</p>
    </section>
  );
}
