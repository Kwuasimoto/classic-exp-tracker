import { useState } from "react";
import type { TranslatorStatus } from "../lib/ipc";

/** The chat translator's Translations view, laid out like a chat: a log that fills the page
 *  (flat lines, no bubbles) and a reply box under it, to write in English and get it in an
 *  installed language to type into the game. The app never types into the game itself.
 *  Reading the chat and translating aren't built yet, so the log holds what's needed first
 *  and Translate is disabled. */
export function LiveTranslations({ status, onSetUp, onTurnOn }: { status: TranslatorStatus | null; onSetUp: () => void; onTurnOn: () => void }) {
  const installed = status?.languages.filter((l) => l.installed) ?? [];
  const [reply, setReply] = useState("");
  const [target, setTarget] = useState<string | null>(null);
  const to = installed.find((l) => l.id === target) ?? installed[0] ?? null;
  const ready = status?.enabled === true && to !== null;

  return (
    <section className="panel chat">
      <div className="chat-log" role="log" aria-label="Chat, translated into English">
        {status === null ? (
          <p className="caption chat-empty">Checking what's installed…</p>
        ) : installed.length === 0 ? (
          <div className="chat-empty">
            <p><b>No translator installed.</b> Install a language in Setup to translate the chat.</p>
            <button className="btn primary" onClick={onSetUp}>Set up a language</button>
          </div>
        ) : !status.enabled ? (
          <div className="chat-empty">
            <p>
              <b>The translator is off.</b> While it's on, it reads the chat and runs the translator on this PC, which takes noticeable processing power and over 1 GB of memory: turn it off when you don't need it.
            </p>
            <button className="btn primary" onClick={onTurnOn}>Turn on</button>
          </div>
        ) : (
          <p className="caption chat-empty">
            On, for {installed.map((l) => l.label).join(", ")}. The chat's lines will show here in English as they're read; reading the chat isn't built yet.
          </p>
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
          placeholder={ready ? "Write in English" : "Write in English to reply"}
          aria-label="Reply in English"
          disabled={!ready}
          onChange={(e) => setReply(e.target.value)}
        />
        <select className="chat-target" aria-label="Translate into" value={to?.id ?? ""} disabled={!ready} onChange={(e) => setTarget(e.target.value)}>
          {installed.length === 0 && <option value="">No language</option>}
          {installed.map((l) => (
            <option key={l.id} value={l.id}>{l.label}</option>
          ))}
        </select>
        <button type="submit" className="btn primary" disabled title="Translating isn't built yet">
          Translate
        </button>
      </form>
      <p className="caption chat-compose-note">
        {ready
          ? "Translating replies isn't built yet. Once it is, the translation shows here to copy into the game's chat."
          : installed.length === 0
            ? "Install a language in Setup to write replies in it."
            : "Turn the translator on to write replies in another language."}
      </p>
    </section>
  );
}
