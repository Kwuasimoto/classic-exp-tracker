import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ItemsPanel } from "../components/ItemsPanel";
import { Ledger } from "../components/Ledger";
import { Pickups } from "../components/Pickups";
import { mesoSeries, RateChart } from "../components/RateChart";
import { formatWhole } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { getLiveChart, setInventoryCorner, setLootEnabled, WINDOWS_MINUTES, type AppError, type Bar, type Corner, type LootStatus, type Tick, type WindowMinutes } from "../lib/ipc";
import { lockOf, lootHeldSince, mesoLines, mesoRates, profitCaption } from "../lib/live";
import type { Messages } from "../locales/en";

const TOAST_MS = 3000;

const CORNERS: readonly Corner[] = ["topRight", "bottomRight", "bottomLeft"];

/** Why meso tracking can't count right now, if it can't. */
function problem(loot: LootStatus, shopping: boolean, m: Messages): string | null {
  if (shopping) return m.mesos.paused;
  if (!loot.found) return m.mesos.notFound(m.mesos.corners[loot.corner].toLowerCase());
  if (!loot.inventory) return m.mesos.totalUnreadable;
  if (!loot.etcTab) return m.mesos.etcTabClosed;
  if (!loot.quickslot) return m.mesos.noPotions;
  return null;
}

/** Meso tracking: mesos picked up, Etc drops at NPC prices and potions used, per hour. */
export function MesoView({ tick, status }: { tick: Tick | null; status: ReactNode }) {
  const m = useMessages();
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

  const why = loot && on ? problem(loot, tick?.shopping === true, m) : null;
  const lock = lockOf(tick, m);
  const rates = mesoRates(bars);
  const held = lootHeldSince(loot);

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          <h1 className="label">{m.mesos.tracking}</h1>
          <span className={why ? "page-title warn" : "page-title"} title={on ? (why ?? undefined) : m.mesos.offTitle}>
            {on ? (why ?? m.mesos.counting) : m.mesos.off}
          </span>
        </div>
        <div className="page-head-actions">
          <div className="segmented" role="group" aria-label={m.mesos.corner}>
            {CORNERS.map((c) => (
              <button key={c} className={c === loot?.corner ? "current" : ""} aria-pressed={c === loot?.corner} disabled={tick === null} onClick={() => void chooseCorner(c)}>
                {m.mesos.corners[c]}
              </button>
            ))}
          </div>
          <button className={on ? "btn" : "btn primary"} disabled={tick === null} onClick={() => void toggle()}>
            {on ? m.common.turnOff : m.common.turnOn}
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
              { title: m.live.profitPerHour, tone: "meso", value: rates === null ? "—" : formatWhole(rates.profit), caption: `${profitCaption(held, m)}${lock ? ` · ${lock.name}` : ""}` },
              { title: m.mesos.mesoTotal, tone: "plain", value: loot?.mesoTotal == null ? "—" : formatWhole(loot.mesoTotal), caption: m.mesos.inInventory },
            ]}
            warning={lock?.away ? m.mesos.lockedAway(lock.name) : held !== null ? m.mesos.heldWarning : null}
            groups={[{ title: m.mesos.perHour, lines: mesoLines(rates, loot, m) }]}
          >
            <Pickups pickedUp={loot?.pickedUp ?? []} />
          </Ledger>
          <div className="page-main">
            <RateChart
              series={mesoSeries(bars, m)}
              fill
              actions={
                <div className="segmented" role="group" aria-label={m.common.chartWindow}>
                  {WINDOWS_MINUTES.map((w) => (
                    <button key={w} className={w === windowMinutes ? "current" : ""} aria-pressed={w === windowMinutes} onClick={() => setWindowMinutes(w)}>
                      {m.time.windowHours(w / 60)}
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
