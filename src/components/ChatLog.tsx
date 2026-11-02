import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useMessages } from "../lib/i18n";
import { chatLines, onChatEnglish, onChatLine, translateLine, type ChatLine, type English } from "../lib/ipc";
import "./chatlog.css";

/** Within this many pixels of the bottom, the log counts as following the newest line. */
const FOLLOW_SLACK = 24;
/** Lines kept, as many as the backend keeps (chat_reader.rs). */
const KEPT = 200;

/** The chat's lines as they're read (chat_reader.rs), newest at the bottom: the backend's
 *  latest when it mounts, then each new one. Like the game's chat, it follows new lines while
 *  it's at the bottom and stays put while the user has scrolled up to read (Jump to newest
 *  takes them back). Each line can be translated into English from its Translate button; the
 *  translation is kept with the line in the backend, so the app and the popout show the same.
 *  `empty` shows until the first line arrives. Used by the Chat translator page and the chat
 *  popout. */
export function ChatLog({ className, empty }: { className?: string; empty: ReactNode }) {
  const t = useMessages().translator;
  const [lines, setLines] = useState<ChatLine[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  // Translate clicks the backend couldn't take (the line is gone, or the call failed), by line.
  const [refused, setRefused] = useState<Record<number, string>>({});

  const translateOne = async (line: ChatLine) => {
    setRefused(({ [line.id]: _, ...rest }) => rest);
    const res = await translateLine(line.id);
    if (!res.success) setRefused((now) => ({ ...now, [line.id]: res.error.message }));
  };

  useLayoutEffect(() => {
    const log = logRef.current;
    if (log && following) log.scrollTop = log.scrollHeight;
  }, [lines, refused, following]);
  const onLogScroll = () => {
    const log = logRef.current;
    if (log) setFollowing(log.scrollHeight - log.scrollTop - log.clientHeight <= FOLLOW_SLACK);
  };

  useEffect(() => {
    let disposed = false;
    void chatLines().then((res) => {
      if (!disposed && res.success) setLines((now) => [...res.data, ...now.filter((l) => !res.data.some((d) => d.id === l.id))]);
    });
    const unlistens: (() => void)[] = [];
    const keep = (res: Awaited<ReturnType<typeof onChatLine>>) => {
      if (!res.success) return;
      if (disposed) res.data();
      else unlistens.push(res.data);
    };
    void onChatLine((res) => {
      if (res.success) setLines((now) => [...now, res.data].slice(-KEPT));
    }).then(keep);
    void onChatEnglish((res) => {
      if (res.success) setLines((now) => now.map((l) => (l.id === res.data.id ? { ...l, english: res.data.english } : l)));
    }).then(keep);
    return () => {
      disposed = true;
      unlistens.forEach((u) => u());
    };
  }, []);

  return (
    <div className={className ? `chat-log ${className}` : "chat-log"} role="log" aria-label={t.log} ref={logRef} onScroll={onLogScroll}>
      {lines.length === 0
        ? empty
        : lines.map((line) => (
            <ChatLineView
              key={line.id}
              line={line}
              kindName={t.chatKinds[line.kind] ?? line.kind}
              refused={refused[line.id]}
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

/** One line read from the game's chat, "talk first": people talking (chat and party) large,
 *  their name above what they said; megaphone ads and crate announcements small and quiet,
 *  with a stripe in their in-game colour; notices set apart. Hovering shows the game's time
 *  (when its timestamps are on), the kind and how sure the reader was, and a Translate button
 *  that asks Gemini for the line in English; the translation shows under the line. */
function ChatLineView({
  line,
  kindName,
  refused,
  onTranslate,
}: {
  line: ChatLine;
  kindName: string;
  refused: string | undefined;
  onTranslate: () => void;
}) {
  const t = useMessages().translator;
  // The game's time, only when the player shows timestamps in the game.
  const title = [line.time, kindName, `${Math.round(line.confidence * 100)}%`].filter(Boolean).join(" · ");
  const label = <span className="sr-only">{kindName}: </span>;
  const english: English | null = line.english;
  const busy = english?.state === "busy";
  const problem = english?.state === "failed" ? english.error : english === null ? refused : undefined;
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
      {english?.state === "done" && (
        <p className="chat-english" title={t.by(english.model)}>
          <span lang="en">{english.text}</span>
        </p>
      )}
      {problem && <p className="chat-english-error" role="alert">{problem}</p>}
      <button className="chat-translate" title={t.translateLineTitle} disabled={busy} onClick={onTranslate}>
        {busy ? t.translating : t.translate}
      </button>
    </div>
  );
}
