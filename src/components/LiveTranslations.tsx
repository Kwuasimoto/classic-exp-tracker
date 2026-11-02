import { useEffect, useState } from "react";
import { useMessages } from "../lib/i18n";
import { REPLY_LANGUAGES, translateReply, type TranslatorStatus } from "../lib/ipc";
import { ChatLog } from "./ChatLog";

/** BCP 47 tags of the translator's language ids, for the translation's `lang`. */
const LANG_TAG: Record<string, string> = { "zh-hans": "zh-Hans", "pt-br": "pt-BR" };

/** A translation still running after this long gets the high-demand note. */
const SLOW_MS = 2000;

type Translated = { text: string; model: string; language: string };

/** The chat translator's Translations view, laid out like a chat: a log that fills the page
 *  (flat lines, no bubbles) and a reply box under it. A reply is translated by Gemini with the
 *  user's key into any of the translator's languages, to copy into the game; the app never
 *  types into the game itself. The log (ChatLog) shows the chat's lines as they're read,
 *  each translatable into English. */
export function LiveTranslations({
  status,
  keySaved,
  onSetUp,
  onTurnOn,
}: {
  status: TranslatorStatus | null;
  keySaved: boolean | null;
  onSetUp: () => void;
  onTurnOn: () => void;
}) {
  const m = useMessages();
  const t = m.translator;
  const [reply, setReply] = useState("");
  const [target, setTarget] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [translated, setTranslated] = useState<Translated | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  // The running translation has taken longer than SLOW_MS.
  const [slow, setSlow] = useState(false);
  const to = REPLY_LANGUAGES.find((id) => id === target) ?? REPLY_LANGUAGES[0];
  const ready = keySaved === true;
  const name = (id: string) => t.languages[id] ?? id;

  useEffect(() => {
    if (!busy) {
      setSlow(false);
      return;
    }
    const timer = setTimeout(() => setSlow(true), SLOW_MS);
    return () => clearTimeout(timer);
  }, [busy]);

  const translate = async () => {
    setBusy(true);
    setCopied(false);
    const res = await translateReply(reply, to);
    setBusy(false);
    if (res.success) {
      setTranslated({ ...res.data, language: to });
      setProblem(null);
    } else {
      setProblem(res.error.message);
    }
  };

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setProblem(null);
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="panel chat">
      {status === null ? (
        <div className="chat-log">
          <p className="caption chat-empty">{t.checking}</p>
        </div>
      ) : !status.downloaded ? (
        <div className="chat-log">
          <div className="chat-empty">
            <p><b>{t.notDownloaded}</b> {t.notDownloadedText}</p>
            <button className="btn primary" onClick={onSetUp}>{t.openSetup}</button>
          </div>
        </div>
      ) : !status.enabled ? (
        <div className="chat-log">
          <div className="chat-empty">
            <p>
              <b>{t.isOff}</b> {t.isOffText}
            </p>
            <button className="btn primary" onClick={onTurnOn}>{m.common.turnOn}</button>
          </div>
        </div>
      ) : (
        <ChatLog empty={<p className="caption chat-empty">{t.onText}</p>} />
      )}

      {translated && (
        <div className="reply-out">
          <span className="caption reply-out-label">{t.inLanguage(name(translated.language))}</span>
          <span className="reply-out-text" lang={LANG_TAG[translated.language] ?? translated.language} title={t.by(translated.model)}>
            {translated.text}
          </span>
          <button className="btn small primary" onClick={() => void copy(translated.text)}>{copied ? t.copied : t.copy}</button>
        </div>
      )}
      <form
        className="chat-compose"
        onSubmit={(e) => {
          e.preventDefault();
          void translate();
        }}
      >
        <input
          className="chat-input"
          value={reply}
          maxLength={500}
          placeholder={ready ? t.replyPlaceholder : t.replyPlaceholderOff}
          aria-label={t.reply}
          disabled={!ready}
          onChange={(e) => setReply(e.target.value)}
        />
        <select className="chat-target" aria-label={t.translateInto} value={to} disabled={!ready} onChange={(e) => setTarget(e.target.value)}>
          {REPLY_LANGUAGES.map((id) => (
            <option key={id} value={id}>{name(id)}</option>
          ))}
        </select>
        <button type="submit" className="btn primary" disabled={!ready || busy || reply.trim() === ""} title={t.translateTitle}>
          {busy ? t.translating : t.translate}
        </button>
      </form>
      {problem !== null ? (
        <p className="caption chat-compose-note warn-text" role="alert">{problem}</p>
      ) : slow ? (
        <p className="caption chat-compose-note warn-text" role="status">{t.highDemand}</p>
      ) : keySaved === false ? (
        <p className="caption chat-compose-note">
          {t.replyNoteNoKey}{" "}
          <button className="btn small" onClick={onSetUp}>{t.addKey}</button>
        </p>
      ) : (
        <p className="caption chat-compose-note">{t.replyNoteReady}</p>
      )}
    </section>
  );
}
