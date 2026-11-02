import { useEffect, useRef, useState } from "react";
import { useMessages } from "../lib/i18n";
import type { Popout } from "../lib/ipc";

/** A pushpin: a popout pinned above the game. */
const PIN = <><line x1="12" y1="17" x2="12" y2="22" /><path d="M9 3h6l-1 6 3 3v2H7v-2l3-3z" /></>;

/** The rail's pin and its menu of the two popouts (rates and chat), each a switch: on while
 *  its popout is open. The menu stays open after a choice, so both can be set;
 *  it closes on a click outside or Escape, like the language menu. */
export function PinMenu({ open: pinned, onToggle }: { open: Record<Popout, boolean>; onToggle: (popout: Popout) => void }) {
  const m = useMessages();
  const a = m.app;
  const [open, setOpen] = useState(false);
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

  const options: Array<{ id: Popout; label: string; title: string }> = [
    { id: "rates", label: a.pinRates, title: pinned.rates ? a.pinOpenTitle : a.pinClosedTitle },
    { id: "chat", label: a.pinChat, title: pinned.chat ? a.pinChatOpenTitle : a.pinChatClosedTitle },
  ];
  const any = pinned.rates || pinned.chat;
  return (
    <div className="rail-menu" ref={box}>
      <button
        className={open ? "rail-btn open" : any ? "rail-btn current" : "rail-btn"}
        aria-label={a.pinMenu}
        title={a.pinMenu}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          {PIN}
        </svg>
      </button>
      {open && (
        <div className="flyout" role="dialog" aria-label={a.pinMenu}>
          {options.map((o) => (
            <button key={o.id} className={pinned[o.id] ? "flyout-option pin-option on" : "flyout-option pin-option"} role="switch" aria-checked={pinned[o.id]} title={o.title} onClick={() => onToggle(o.id)}>
              {o.label}
              <span className="switch" aria-hidden="true" />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
