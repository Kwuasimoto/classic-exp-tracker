import { Fragment, useState } from "react";
import { PatchView } from "./PatchView";
import { useMessages } from "../lib/i18n";
import { setMainPotion, type AppError, type LootStatus, type PotionSlot } from "../lib/ipc";

const SLOTS: ReadonlyArray<{ slot: PotionSlot; key: "mainHp" | "mainMp"; label: string }> = [
  { slot: "hp", key: "mainHp", label: "HP" },
  { slot: "mp", key: "mainMp", label: "MP" },
];

/** The main HP and MP potions (the only ones whose use costs mesos), how many of each are on
 *  the quickslot, and which quickslot layout is being read. Click a line to choose that pot
 *  from the ones on the quickslot (Live). */
export function ActivePots({ loot, onError, onDone }: { loot: LootStatus; onError: (e: AppError) => void; onDone: (msg: string) => void }) {
  const m = useMessages();
  const [open, setOpen] = useState<PotionSlot | null>(null);

  const choose = async (slot: PotionSlot, itemId: number | null) => {
    const res = await setMainPotion(slot, itemId);
    if (!res.success) return onError(res.error);
    setOpen(null);
    onDone(itemId === null ? m.pots.cleared(slot.toUpperCase()) : m.pots.set(slot.toUpperCase()));
  };

  const layout = loot.quickslotLayout;
  return (
    <div className="ledger-group pots">
      <div className="ledger-group-head">
        <span className="label">{m.pots.title}</span>
        {layout === null ? (
          <span className="pots-via warn" title={m.pots.notReadTitle}>
            {m.pots.notRead}
          </span>
        ) : (
          <span className="pots-via" title={m.pots.layoutTitle}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              {layout === "keyboard" ? (
                <><rect x="2" y="6" width="20" height="12" rx="2" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" /></>
              ) : (
                <><rect x="2" y="6" width="20" height="12" rx="6" /><path d="M6 12h4M8 10v4M15 12h.01M18 10h.01" /></>
              )}
            </svg>
            {layout === "keyboard" ? m.pots.keyboard : m.pots.controller}
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
              title={main?.name === null ? m.pots.unnamedTitle : m.pots.choose(label)}
              onClick={() => setOpen(open === slot ? null : slot)}
            >
              <span className="pot-slot">{label}</span>
              {pot ? <PatchView patch={pot.patch} zoom={2} /> : <span className="pot-empty" />}
              <span className={main !== null && main.name === null ? "pot-name unknown" : "pot-name"}>
                {main === null ? m.pots.choose(label) : (main.name ?? m.pickups.unnamed)}
              </span>
              {main !== null && <span className="pot-count" title={pot ? m.pots.onQuickslot : m.pots.notOnQuickslot}>{pot ? `×${pot.count}` : "—"}</span>}
              <span className="pot-chevron" aria-hidden="true">›</span>
            </button>
            {open === slot && (
              <div className="pot-chooser">
                <div className="caption">{loot.quickslotPots.length === 0 ? m.pots.noneRead : m.pots.pick(label)}</div>
                <div className="pot-picks">
                  {loot.quickslotPots.map((p) => (
                    <button
                      key={p.itemId}
                      className={main?.itemId === p.itemId ? "pot-pick current" : "pot-pick"}
                      aria-pressed={main?.itemId === p.itemId}
                      aria-label={m.pots.potLabel(p.name ?? m.common.unnamed, p.count)}
                      title={p.name ?? m.pots.pickUnnamedTitle}
                      onClick={() => void choose(slot, p.itemId)}
                    >
                      <PatchView patch={p.patch} zoom={2} />
                      <span className="pot-pick-count">{p.count}</span>
                    </button>
                  ))}
                  {main !== null && <button className="btn small" onClick={() => void choose(slot, null)}>{m.pots.none}</button>}
                </div>
              </div>
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
