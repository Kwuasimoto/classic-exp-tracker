import { useCallback, useEffect, useState } from "react";
import { PatchView } from "./PatchView";
import { Scroller } from "./Scroller";
import { useMessages } from "../lib/i18n";
import { deleteItem, getItems, ignoreItem, itemNames, nameItem, setMainPotion, type AppError, type Item, type ItemKind, type PotionSlot } from "../lib/ipc";

type Tab = "etc" | "use" | "ignored";
const TABS: readonly Tab[] = ["etc", "use", "ignored"];

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
  const m = useMessages();
  const it = m.items;
  const kindLabel = (kind: ItemKind) => it.tabs[kind];
  const [items, setItems] = useState<Item[]>([]);
  const [names, setNames] = useState<Record<ItemKind, string[]>>({ etc: [], use: [] });
  const [tab, setTab] = useState<Tab>("etc");
  const [selected, setSelected] = useState<number | null>(null);
  const [typed, setTyped] = useState("");
  // Deleting asks first.
  const [deleting, setDeleting] = useState(false);

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
    setDeleting(false);
    setTyped(i.name ?? "");
  };

  const save = async () => {
    if (!item) return;
    const name = typed.trim();
    if (!names[item.kind].includes(name)) return onError({ kind: "invalid", message: it.pickFromList(kindLabel(item.kind)) });
    const res = await nameItem(item.id, name);
    if (!res.success) return onError(res.error);
    onDone(it.named(name));
    setSelected(null);
    await load();
  };

  const setIgnored = async (ignored: boolean) => {
    if (!item) return;
    const res = await ignoreItem(item.id, ignored);
    if (!res.success) return onError(res.error);
    onDone(ignored ? it.itemIgnored : it.itemTracked);
    setSelected(null);
    await load();
  };

  const remove = async () => {
    if (!item) return;
    const res = await deleteItem(item.id);
    setDeleting(false);
    if (!res.success) return onError(res.error);
    onDone(it.itemDeleted);
    setSelected(null);
    await load();
  };

  const toggleMain = async (slot: PotionSlot) => {
    if (!item) return;
    const res = await setMainPotion(slot, main[slot] === item.id ? null : item.id);
    if (!res.success) return onError(res.error);
    onDone(main[slot] === item.id ? it.mainCleared(slot.toUpperCase()) : it.mainSet(slot.toUpperCase()));
  };
  const mainTag = (id: number) => (main.hp === id ? "HP" : main.mp === id ? "MP" : null);

  if (items.length === 0) return null;
  return (
    <section className="panel">
      <h2>{it.title}</h2>
      <div className="item-tabs">
      <div className="row" style={{ gap: 4, flexWrap: "nowrap" }} role="tablist">
        {TABS.map((t) => {
          const toName = t === "ignored" ? 0 : inTab(t).filter((i) => i.name === null).length;
          return (
            <button
              key={t}
              role="tab"
              aria-selected={t === tab}
              className={t === tab ? "item-tab current" : "item-tab"}
              onClick={() => {
                setTab(t);
                setSelected(null);
              }}
            >
              {it.tabs[t]} {inTab(t).length}
              {toName > 0 && <span className="to-name" title={it.toName(toName)}>{toName}</span>}
            </button>
          );
        })}
      </div>
      <span style={{ flex: 1 }} />
      <ul className="item-legend" aria-label={it.legend}>
        <li><span className="item-key" />{it.counted}</li>
        <li><span className="item-key unnamed" /><b className="unknown">?</b> {it.needsName}</li>
        <li><span className="item-key ignored" />{it.ignored}</li>
      </ul>
      </div>
      {shown.length === 0 ? (
        <p className="caption">{tab === "ignored" ? it.noIgnored : it.noneInTab}</p>
      ) : (
        <Scroller className="item-grid" label={it.title}>
          {shown.map((i) => (
            <button
              key={i.id}
              className={["item-slot", i.name === null && !i.ignored ? "unnamed" : "", i.ignored ? "ignored" : "", i.id === selected ? "current" : ""].join(" ")}
              title={i.name ?? it.unnamedTitle}
              aria-label={i.name ?? it.unnamedItem}
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
            placeholder={it.namePlaceholder}
            aria-label={it.itemName}
            autoFocus
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
              if (e.key === "Escape") setSelected(null);
            }}
          />
          <button className="btn small primary" onClick={() => void save()}>{m.common.save}</button>
          <div className="item-editor-actions">
          {item.kind === "use" && !item.ignored &&
            (["hp", "mp"] as const).map((slot) => (
              <button
                key={slot}
                className={main[slot] === item.id ? "btn small current" : "btn small"}
                aria-pressed={main[slot] === item.id}
                title={it.mainTitle(slot.toUpperCase())}
                onClick={() => void toggleMain(slot)}
              >
                {it.main(slot.toUpperCase())}
              </button>
            ))}
          {item.ignored ? (
            <button className="btn small" onClick={() => void setIgnored(false)}>{it.unignore}</button>
          ) : (
            <button className="btn small" title={it.ignoreTitle} onClick={() => void setIgnored(true)}>{it.ignore}</button>
          )}
          {deleting ? (
            <>
              <button className="btn small danger" onClick={() => void remove()}>{m.common.delete}</button>
              <button className="btn small" onClick={() => setDeleting(false)}>{m.common.keep}</button>
            </>
          ) : (
            <button className="btn small" title={it.deleteItemTitle} onClick={() => setDeleting(true)}>{m.common.delete}</button>
          )}
          </div>
        </div>
      )}
    </section>
  );
}
