import { useCallback, useEffect, useState } from "react";
import { PatchView } from "./PatchView";
import { Scroller } from "./Scroller";
import { getItems, ignoreItem, itemNames, nameItem, setMainPotion, type AppError, type Item, type ItemKind, type PotionSlot } from "../lib/ipc";

type Tab = "etc" | "use" | "ignored";
const TABS: ReadonlyArray<{ id: Tab; label: string }> = [
  { id: "etc", label: "Etc" },
  { id: "use", label: "Use" },
  { id: "ignored", label: "Ignored" },
];
const kindLabel = (kind: ItemKind) => (kind === "etc" ? "Etc" : "Use");

/** Every item seen on screen, laid out like the game's inventory: a tab per kind and one for
 *  ignored items, a square per item. Unnamed items count for nothing until named from the
 *  item list; ignored ones are recognised but never counted. Click a square to name it,
 *  rename it, ignore it or track it again. */
export function ItemsPanel({
  unnamed,
  main,
  onError,
  onDone,
}: {
  unnamed: number;
  /** The main HP and MP items' ids: the only potions counted as used. */
  main: Record<PotionSlot, number | null>;
  onError: (e: AppError) => void;
  onDone: (msg: string) => void;
}) {
  const [items, setItems] = useState<Item[]>([]);
  const [names, setNames] = useState<Record<ItemKind, string[]>>({ etc: [], use: [] });
  const [tab, setTab] = useState<Tab>("etc");
  const [selected, setSelected] = useState<number | null>(null);
  const [typed, setTyped] = useState("");

  // The item list's names, once.
  useEffect(() => {
    for (const kind of ["etc", "use"] as const) {
      void itemNames(kind).then((res) => {
        if (res.success) setNames((n) => ({ ...n, [kind]: res.data }));
        else onError(res.error);
      });
    }
  }, [onError]);

  const load = useCallback(async () => {
    const res = await getItems();
    if (res.success) setItems(res.data);
    else onError(res.error);
  }, [onError]);

  // Reload when the number of items to name changes.
  useEffect(() => {
    void load();
  }, [unnamed, load]);

  const inTab = (t: Tab) => items.filter((i) => (t === "ignored" ? i.ignored : !i.ignored && i.kind === t));
  const shown = inTab(tab);
  const item = items.find((i) => i.id === selected) ?? null;

  const select = (i: Item) => {
    setSelected(i.id === selected ? null : i.id);
    setTyped(i.name ?? "");
  };

  const save = async () => {
    if (!item) return;
    const name = typed.trim();
    if (!names[item.kind].includes(name)) return onError({ kind: "invalid", message: `Pick a name from the ${kindLabel(item.kind)} item list` });
    const res = await nameItem(item.id, name);
    if (!res.success) return onError(res.error);
    onDone(`${name} named`);
    setSelected(null);
    await load();
  };

  const setIgnored = async (ignored: boolean) => {
    if (!item) return;
    const res = await ignoreItem(item.id, ignored);
    if (!res.success) return onError(res.error);
    onDone(ignored ? "Item ignored" : "Item tracked again");
    setSelected(null);
    await load();
  };

  const toggleMain = async (slot: PotionSlot) => {
    if (!item) return;
    const res = await setMainPotion(slot, main[slot] === item.id ? null : item.id);
    if (!res.success) return onError(res.error);
    onDone(main[slot] === item.id ? `Main ${slot.toUpperCase()} cleared` : `Main ${slot.toUpperCase()} potion set`);
  };
  const mainTag = (id: number) => (main.hp === id ? "HP" : main.mp === id ? "MP" : null);

  if (items.length === 0) return null;
  return (
    <section className="panel">
      <h2>Items</h2>
      <div className="item-tabs">
      <div className="row" style={{ gap: 4, flexWrap: "nowrap" }} role="tablist">
        {TABS.map((t) => {
          const toName = t.id === "ignored" ? 0 : inTab(t.id).filter((i) => i.name === null).length;
          return (
            <button
              key={t.id}
              role="tab"
              aria-selected={t.id === tab}
              className={t.id === tab ? "item-tab current" : "item-tab"}
              onClick={() => {
                setTab(t.id);
                setSelected(null);
              }}
            >
              {t.label} {inTab(t.id).length}
              {toName > 0 && <span className="to-name" title={`${toName} to name`}>{toName}</span>}
            </button>
          );
        })}
      </div>
      <span style={{ flex: 1 }} />
      <ul className="item-legend" aria-label="Legend">
        <li><span className="item-key" />Counted</li>
        <li><span className="item-key unnamed" /><b className="unknown">?</b> Needs a name</li>
        <li><span className="item-key ignored" />Ignored</li>
      </ul>
      </div>
      {shown.length === 0 ? (
        <p className="caption">{tab === "ignored" ? "No ignored items." : "No items seen in this tab yet."}</p>
      ) : (
        <Scroller className="item-grid" label="Items">
          {shown.map((i) => (
            <button
              key={i.id}
              className={["item-slot", i.name === null && !i.ignored ? "unnamed" : "", i.ignored ? "ignored" : "", i.id === selected ? "current" : ""].join(" ")}
              title={i.name ?? "Unnamed: click to name it"}
              aria-label={i.name ?? "Unnamed item"}
              aria-pressed={i.id === selected}
              onClick={() => select(i)}
            >
              <PatchView patch={i.patch} />
              {i.name === null && !i.ignored && <span className="unknown-badge" aria-hidden="true">?</span>}
              {mainTag(i.id) && <span className="main-tag">{mainTag(i.id)}</span>}
            </button>
          ))}
        </Scroller>
      )}
      {item && (
        <div className="item-editor">
          <span className="item-slot static">
            <PatchView patch={item.patch} />
          </span>
          <span className="label">{kindLabel(item.kind)}</span>
          <datalist id="item-names">{names[item.kind].map((n) => <option key={n} value={n} />)}</datalist>
          <input
            list="item-names"
            placeholder="Item name, from the item list"
            aria-label="Item name"
            autoFocus
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
              if (e.key === "Escape") setSelected(null);
            }}
          />
          <button className="btn small primary" onClick={() => void save()}>Save</button>
          <div className="item-editor-actions">
          {item.kind === "use" && !item.ignored &&
            (["hp", "mp"] as const).map((slot) => (
              <button
                key={slot}
                className={main[slot] === item.id ? "btn small current" : "btn small"}
                aria-pressed={main[slot] === item.id}
                title={`Count this as your main ${slot.toUpperCase()} potion: only the main HP and MP potions count as potions used`}
                onClick={() => void toggleMain(slot)}
              >
                Main {slot.toUpperCase()}
              </button>
            ))}
          {item.ignored ? (
            <button className="btn small" onClick={() => void setIgnored(false)}>Unignore</button>
          ) : (
            <button className="btn small" title="Don't track this item" onClick={() => void setIgnored(true)}>Ignore</button>
          )}
          </div>
        </div>
      )}
    </section>
  );
}
