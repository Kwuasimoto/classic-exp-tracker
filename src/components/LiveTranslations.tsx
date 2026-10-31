import { useState } from "react";
import { useMessages } from "../lib/i18n";
import type { TranslatorStatus } from "../lib/ipc";

/** The chat translator's Translations view, laid out like a chat: a log that fills the page
 *  (flat lines, no bubbles) and a reply box under it, to write in English and get it in an
 *  installed language to type into the game. The app never types into the game itself.
 *  Reading the chat and translating aren't built yet, so the log holds what's needed first
 *  and Translate is disabled. */
export function LiveTranslations({ status, onSetUp, onTurnOn }: { status: TranslatorStatus | null; onSetUp: () => void; onTurnOn: () => void }) {
  const m = useMessages();
  const t = m.translator;
  const installed = status?.languages.filter((l) => l.installed) ?? [];
  const [reply, setReply] = useState("");
  const [target, setTarget] = useState<string | null>(null);
  const to = installed.find((l) => l.id === target) ?? installed[0] ?? null;
  const ready = status?.enabled === true && to !== null;
  const name = (id: string) => t.languages[id] ?? id;

  return (
    <section className="panel chat">
      <div className="chat-log" role="log" aria-label={t.log}>
        {status === null ? (
          <p className="caption chat-empty">{t.checking}</p>
        ) : installed.length === 0 ? (
          <div className="chat-empty">
            <p><b>{t.noneInstalled}</b> {t.noneInstalledText}</p>
            <button className="btn primary" onClick={onSetUp}>{t.setUp}</button>
          </div>
        ) : !status.enabled ? (
          <div className="chat-empty">
            <p>
              <b>{t.isOff}</b> {t.isOffText}
            </p>
            <button className="btn primary" onClick={onTurnOn}>{m.common.turnOn}</button>
          </div>
        ) : (
          <p className="caption chat-empty">{t.onFor(installed.map((l) => name(l.id)).join(", "))}</p>
        )}
      </div>

      <form
        className="chat-compose"
        onSubmit={(e) => {
          e.preventDefault();
        }}
      >
        <input
          className="chat-input"
          value={reply}
          placeholder={ready ? t.replyPlaceholder : t.replyPlaceholderOff}
          aria-label={t.reply}
          disabled={!ready}
          onChange={(e) => setReply(e.target.value)}
        />
        <select className="chat-target" aria-label={t.translateInto} value={to?.id ?? ""} disabled={!ready} onChange={(e) => setTarget(e.target.value)}>
          {installed.length === 0 && <option value="">{t.noLanguage}</option>}
          {installed.map((l) => (
            <option key={l.id} value={l.id}>{name(l.id)}</option>
          ))}
        </select>
        <button type="submit" className="btn primary" disabled title={t.translateTitle}>
          {t.translate}
        </button>
      </form>
      <p className="caption chat-compose-note">{ready ? t.replyNoteReady : installed.length === 0 ? t.replyNoteNone : t.replyNoteOff}</p>
    </section>
  );
}
