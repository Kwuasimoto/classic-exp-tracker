import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ItemsPanel } from "../components/ItemsPanel";
import { Ledger } from "../components/Ledger";
import { Pickups } from "../components/Pickups";
import { mesoSeries, RateChart } from "../components/RateChart";
import { formatWhole } from "../lib/format";
import { getLiveChart, setInventoryCorner, setLootEnabled, WINDOWS_MINUTES, type AppError, type Bar, type Corner, type LootStatus, type Tick, type WindowMinutes } from "../lib/ipc";
import { lockOf, mesoLines, mesoRates } from "../lib/live";

const TOAST_MS = 3000;

const CORNERS: ReadonlyArray<{ id: Corner; label: string }> = [
  { id: "topRight", label: "Top right" },
  { id: "bottomRight", label: "Bottom right" },
  { id: "bottomLeft", label: "Bottom left" },
];
const cornerName = (c: Corner) => (CORNERS.find((k) => k.id === c)?.label ?? c).toLowerCase();

/** Why meso tracking can't count right now, if it can't. */
function problem(loot: LootStatus, shopping: boolean): string | null {
  if (shopping) return "Paused while shopping";
  if (!loot.found) return `Inventory out of place: move it into the ${cornerName(loot.corner)} corner`;
  if (!loot.inventory) return "The inventory's meso total doesn't read";
  if (!loot.etcTab) return "Etc drops count while the Etc tab is open";
  if (!loot.quickslot) return "No potions found on the quickslot";
  return null;
}

/** Meso tracking: mesos picked up, Etc drops at NPC prices and potions used, per hour. */
export function MesoView({ tick, status }: { tick: Tick | null; status: ReactNode }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(WINDOWS_MINUTES[0]);
  const [error, setError] = useState<AppError | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  // Chart requests go out every second and can finish out of order: keep only the newest.
  const latestRequest = useRef(0);
  const loot = tick?.loot ?? null;
  const on = loot?.enabled === true;

  const refreshChart = useCallback(async () => {
    const request = ++latestRequest.current;
    const res = await getLiveChart(windowMinutes);
    if (request !== latestRequest.current) return;
    if (res.success) setBars(res.data);
    else setError(res.error);
  }, [windowMinutes]);

  useEffect(() => {
    void refreshChart();
  }, [tick, refreshChart]);

  useEffect(() => {
    if (toast === null) return;
    const timer = setTimeout(() => setToast(null), TOAST_MS);
    return () => clearTimeout(timer);
  }, [toast]);

  const toggle = async () => {
    const res = await setLootEnabled(!on);
    if (!res.success) return setError(res.error);
    setError(null);
  };

  const chooseCorner = async (corner: Corner) => {
    const res = await setInventoryCorner(corner);
    if (!res.success) return setError(res.error);
    setError(null);
  };

  const why = loot && on ? problem(loot, tick?.shopping === true) : null;
  const lock = lockOf(tick);
  const rates = mesoRates(bars);

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          <span className="label">Meso tracking</span>
          <span className={why ? "page-title warn" : "page-title"} title={on ? (why ?? undefined) : "Reads the inventory's meso total, Etc stacks and quickslot potions. Uses more processing than EXP tracking."}>
            {on ? (why ?? "Counting") : "Off"}
          </span>
        </div>
        <div className="page-head-actions">
          <div className="segmented" role="group" aria-label="Inventory corner">
            {CORNERS.map((c) => (
              <button key={c.id} className={c.id === loot?.corner ? "current" : ""} aria-pressed={c.id === loot?.corner} disabled={tick === null} onClick={() => void chooseCorner(c.id)}>
                {c.label}
              </button>
            ))}
          </div>
          <button className={on ? "btn" : "btn primary"} disabled={tick === null} onClick={() => void toggle()}>
            {on ? "Turn off" : "Turn on"}
          </button>
          {status}
        </div>
      </section>
      {error && <p className="panel error">{error.message}</p>}
      {toast && <div className="toast" role="status">{toast}</div>}

      {on && (
        <div className="page-body">
          <Ledger
            headlines={[
              { title: "Profit / hr", tone: "meso", value: rates === null ? "—" : formatWhole(rates.profit), caption: `after potions${lock ? ` · ${lock.name}` : ""}` },
              { title: "Meso total", tone: "plain", value: loot?.mesoTotal == null ? "—" : formatWhole(loot.mesoTotal), caption: "in the inventory" },
            ]}
            warning={lock?.away ? `Locked to ${lock.name} on the Live page: mesos here aren't counted` : null}
            groups={[{ title: "Mesos per hour", lines: mesoLines(rates, loot) }]}
          >
            <Pickups pickedUp={loot?.pickedUp ?? []} />
          </Ledger>
          <div className="page-main">
            <RateChart
              series={mesoSeries(bars)}
              fill
              actions={
                <div className="segmented" role="group" aria-label="Chart window">
                  {WINDOWS_MINUTES.map((m) => (
                    <button key={m} className={m === windowMinutes ? "current" : ""} aria-pressed={m === windowMinutes} onClick={() => setWindowMinutes(m)}>
                      {m / 60}H
                    </button>
                  ))}
                </div>
              }
            />
            {loot && (
              <ItemsPanel
                unnamed={loot.unnamedItems}
                main={{ hp: loot.mainHp?.itemId ?? null, mp: loot.mainMp?.itemId ?? null }}
                onError={setError}
                onDone={setToast}
              />
            )}
          </div>
        </div>
      )}
    </>
  );
}
