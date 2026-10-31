import { Fragment, useState } from "react";
import { PatchView } from "./PatchView";
import { setMainPotion, type AppError, type LootStatus, type PotionSlot } from "../lib/ipc";

const SLOTS: ReadonlyArray<{ slot: PotionSlot; key: "mainHp" | "mainMp"; label: string }> = [
  { slot: "hp", key: "mainHp", label: "HP" },
  { slot: "mp", key: "mainMp", label: "MP" },
];

/** The main HP and MP potions (the only ones whose use costs mesos), how many of each are on
 *  the quickslot, and which quickslot layout is being read. Click a line to choose that pot
 *  from the ones on the quickslot (Live). */
export function ActivePots({ loot, onError, onDone }: { loot: LootStatus; onError: (e: AppError) => void; onDone: (msg: string) => void }) {
  const [open, setOpen] = useState<PotionSlot | null>(null);

  const choose = async (slot: PotionSlot, itemId: number | null) => {
    const res = await setMainPotion(slot, itemId);
    if (!res.success) return onError(res.error);
    setOpen(null);
    onDone(itemId === null ? `Main ${slot.toUpperCase()} cleared` : `Main ${slot.toUpperCase()} pot set`);
  };

  const layout = loot.quickslotLayout;
  return (
    <div className="ledger-group pots">
      <div className="ledger-group-head">
        <span className="label">Active pots</span>
        {layout === null ? (
          <span className="pots-via warn" title="The quickslot didn't read this second: it hides while the chat is open, and a cursor over it can cover the counts">
            Quickslot not read
          </span>
        ) : (
          <span className="pots-via" title="The quickslot layout the app is reading">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              {layout === "keyboard" ? (
                <><rect x="2" y="6" width="20" height="12" rx="2" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" /></>
              ) : (
                <><rect x="2" y="6" width="20" height="12" rx="6" /><path d="M6 12h4M8 10v4M15 12h.01M18 10h.01" /></>
              )}
            </svg>
            {layout === "keyboard" ? "Keyboard layout" : "Controller layout"}
          </span>
        )}
      </div>
      {SLOTS.map(({ slot, key, label }) => {
        const main = loot[key];
        const pot = main === null ? undefined : loot.quickslotPots.find((p) => p.itemId === main.itemId);
        return (
          <Fragment key={slot}>
            <button
              className={["pot-line", open === slot ? "open" : "", main === null ? "unset" : ""].join(" ")}
              aria-expanded={open === slot}
              title={main?.name === null ? "Unnamed: name it under Mesos › Items › Use, it costs nothing until then" : `Choose your ${label} pot`}
              onClick={() => setOpen(open === slot ? null : slot)}
            >
              <span className="pot-slot">{label}</span>
              {pot ? <PatchView patch={pot.patch} zoom={2} /> : <span className="pot-empty" />}
              <span className={main !== null && main.name === null ? "pot-name unknown" : "pot-name"}>
                {main === null ? `Choose your ${label} pot` : (main.name ?? "? Unnamed")}
              </span>
              {main !== null && <span className="pot-count" title={pot ? "On your quickslot" : "Not on your quickslot"}>{pot ? `×${pot.count}` : "—"}</span>}
              <span className="pot-chevron" aria-hidden="true">›</span>
            </button>
            {open === slot && (
              <div className="pot-chooser">
                <div className="caption">{loot.quickslotPots.length === 0 ? "No pots read on your quickslot yet." : `On your quickslot: pick your ${label} pot`}</div>
                <div className="pot-picks">
                  {loot.quickslotPots.map((p) => (
                    <button
                      key={p.itemId}
                      className={main?.itemId === p.itemId ? "pot-pick current" : "pot-pick"}
                      aria-pressed={main?.itemId === p.itemId}
                      aria-label={`${p.name ?? "Unnamed"}, ${p.count} on the quickslot`}
                      title={p.name ?? "Unnamed: name it under Mesos › Items › Use"}
                      onClick={() => void choose(slot, p.itemId)}
                    >
                      <PatchView patch={p.patch} zoom={2} />
                      <span className="pot-pick-count">{p.count}</span>
                    </button>
                  ))}
                  {main !== null && <button className="btn small" onClick={() => void choose(slot, null)}>None</button>}
                </div>
              </div>
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
