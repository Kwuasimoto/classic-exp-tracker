import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useMessages } from "../lib/i18n";
import { chatLines, onChatLine, translateLine, type ChatLine } from "../lib/ipc";
import "./chatlog.css";

/** Within this many pixels of the bottom, the log counts as following the newest line. */
const FOLLOW_SLACK = 24;
/** Lines kept, as many as the backend keeps (chat_reader.rs). */
const KEPT = 200;

/** The chat's lines as they're read (chat_reader.rs), newest at the bottom: the backend's
 *  latest when it mounts, then each new one. Like the game's chat, it follows new lines while
 *  it's at the bottom and stays put while the user has scrolled up to read (Jump to newest
 *  takes them back). Each line can be translated into English from its Translate button.
 *  `empty` shows until the first line arrives. Used by the Chat translator page and the chat
 *  popout. */
export function ChatLog({ className, empty }: { className?: string; empty: ReactNode }) {
  const t = useMessages().translator;
  const [lines, setLines] = useState<ChatLine[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  // English translations of lines, asked for one at a time from each line's Translate button.
  const [english, setEnglish] = useState<Record<string, LineTranslation>>({});

  const translateOne = async (line: ChatLine) => {
    const id = lineId(line);
    setEnglish((now) => ({ ...now, [id]: { busy: true } }));
    const res = await translateLine(line.text);
    setEnglish((now) => ({ ...now, [id]: res.success ? { text: res.data.text, model: res.data.model } : { error: res.error.message } }));
  };

  useLayoutEffect(() => {
    const log = logRef.current;
    if (log && following) log.scrollTop = log.scrollHeight;
  }, [lines, english, following]);
  const onLogScroll = () => {
    const log = logRef.current;
    if (log) setFollowing(log.scrollHeight - log.scrollTop - log.clientHeight <= FOLLOW_SLACK);
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void chatLines().then((res) => {
      if (!disposed && res.success) setLines((now) => [...res.data, ...now.filter((l) => !res.data.some((d) => lineId(d) === lineId(l)))]);
    });
    void onChatLine((res) => {
      if (res.success) setLines((now) => [...now, res.data].slice(-KEPT));
    }).then((res) => {
      if (!res.success) return;
      if (disposed) res.data();
      else unlisten = res.data;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <div className={className ? `chat-log ${className}` : "chat-log"} role="log" aria-label={t.log} ref={logRef} onScroll={onLogScroll}>
      {lines.length === 0
        ? empty
        : lines.map((line, i) => (
            <ChatLineView
              key={`${line.t}-${i}`}
              line={line}
              kindName={t.chatKinds[line.kind] ?? line.kind}
              english={english[lineId(line)]}
              onTranslate={() => void translateOne(line)}
            />
          ))}
      {!following && lines.length > 0 && (
        <div className="chat-jump">
          <button onClick={() => setFollowing(true)}>{t.jumpToNewest}</button>
        </div>
      )}
    </div>
  );
}

/** Item names in a line ("[Arc Staff]"), shown in the game's item gold. */
function withItems(text: string): ReactNode[] {
  return text.split(/(\[[^\]]+\])/).map((part, i) => (i % 2 === 1 ? <span key={i} className="chat-item">{part}</span> : part));
}

/** A line's English translation: asked for, done (with the model that made it), or failed. */
type LineTranslation = { busy: true } | { text: string; model: string } | { error: string };

/** The same message keeps its time and text, whichever list it came from. */
const lineId = (line: ChatLine) => `${line.t}|${line.text}`;

/** One line read from the game's chat, "talk first": people talking (chat and party) large,
 *  their name above what they said; megaphone ads and crate announcements small and quiet,
 *  with a stripe in their in-game colour; notices set apart. Hovering shows the kind and how
 *  sure the reader was, and a Translate button that asks Gemini for the line in English; the
 *  translation shows under the line. */
function ChatLineView({
  line,
  kindName,
  english,
  onTranslate,
}: {
  line: ChatLine;
  kindName: string;
  english: LineTranslation | undefined;
  onTranslate: () => void;
}) {
  const t = useMessages().translator;
  const title = `${kindName} · ${Math.round(line.confidence * 100)}%`;
  const label = <span className="sr-only">{kindName}: </span>;
  const busy = english !== undefined && "busy" in english;
  let body: ReactNode;
  if (line.kind === "normal" || line.kind === "party") {
    body = (
      <p className={`chat-talk chat-${line.kind}`} title={title}>
        {label}
        <b className="chat-sender">{line.sender}</b>
        <span className="chat-said">{line.text}</span>
      </p>
    );
  } else if (line.kind === "notice") {
    body = (
      <p className="chat-notice" title={title}>
        {label}
        {line.text}
      </p>
    );
  } else {
    body = (
      <p className={`chat-ad chat-${line.kind}`} title={title}>
        {label}
        {line.sender && <b className="chat-sender">{line.sender}</b>}
        {line.channel !== null && <span className="chat-channel">CH{String(line.channel).padStart(2, "0")}</span>}
        <span className="chat-said">{withItems(line.text)}</span>
      </p>
    );
  }
  return (
    <div className={busy ? "chat-entry busy" : "chat-entry"}>
      {body}
      {english !== undefined && "text" in english && (
        <p className="chat-english" title={t.by(english.model)}>
          <span lang="en">{english.text}</span>
        </p>
      )}
      {english !== undefined && "error" in english && <p className="chat-english-error" role="alert">{english.error}</p>}
      <button className="chat-translate" title={t.translateLineTitle} disabled={busy} onClick={onTranslate}>
        {busy ? t.translating : t.translate}
      </button>
    </div>
  );
}
