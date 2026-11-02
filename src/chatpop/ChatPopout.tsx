import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState, type MouseEvent } from "react";
import { ChatLog } from "../components/ChatLog";
import { useMessages } from "../lib/i18n";
import { placeAndShow } from "./place";

/** Where a resize can start: each edge and corner of the window. */
const EDGES = [
  ["n", "North"],
  ["s", "South"],
  ["e", "East"],
  ["w", "West"],
  ["ne", "NorthEast"],
  ["nw", "NorthWest"],
  ["se", "SouthEast"],
  ["sw", "SouthWest"],
] as const;

/** The chat popout: the chat log alone, in a frameless window pinned above the game, sized
 *  and placed to sit over the game's chat box if the user wants (commands::set_popout). It's
 *  hidden from the screen capture, so the chat reader still reads the game's chat under it.
 *  Pointing at it shows a grip to drag it and ✕ to close it; every edge resizes it. */
export function ChatPopout() {
  const m = useMessages();
  const c = m.chatPopout;
  const [problem, setProblem] = useState<string | null>(null);
  const fail = (e: unknown) => setProblem(`window: ${String(e)}`);

  useEffect(() => {
    void placeAndShow().then((res) => setProblem(res.success ? null : `window: ${res.error}`));
  }, []);

  const drag = (e: MouseEvent) => {
    if (e.button === 0) void getCurrentWindow().startDragging().catch(fail);
  };

  return (
    <main className="chatpop">
      <h1 className="sr-only">{c.heading}</h1>
      <ChatLog className="chatpop-log" empty={<p className="chatpop-empty">{c.empty}</p>} />
      {problem && <p className="chatpop-problem" role="alert">{problem}</p>}
      <div className="chatpop-grip" title={c.move} onMouseDown={drag} />
      <button className="chatpop-close" aria-label={c.close} title={c.close} onClick={() => void getCurrentWindow().close().catch(fail)}>
        ✕
      </button>
      {EDGES.map(([edge, direction]) => (
        <div
          key={edge}
          className={`chatpop-edge ${edge}`}
          aria-hidden="true"
          onMouseDown={(e) => {
            if (e.button === 0) void getCurrentWindow().startResizeDragging(direction).catch(fail);
          }}
        />
      ))}
    </main>
  );
}
