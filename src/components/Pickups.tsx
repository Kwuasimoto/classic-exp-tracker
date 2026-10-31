import { formatWhole } from "../lib/format";
import { useMessages } from "../lib/i18n";
import type { PickedUp } from "../lib/ipc";
import { PatchView } from "./PatchView";
import { Scroller } from "./Scroller";

/** The Etc picked up this session, one line per item, most valuable first (Live, Mesos). */
export function Pickups({ pickedUp }: { pickedUp: PickedUp[] }) {
  const m = useMessages();
  return (
    <>
      <div className="label ledger-heading">{m.pickups.title}</div>
      <Scroller className="pickups" label={m.pickups.title}>
        {pickedUp.length === 0 ? (
          <p className="caption">{m.pickups.empty}</p>
        ) : (
          <table>
            <colgroup>
              <col style={{ width: 28 }} />
              <col />
              <col style={{ width: 44 }} />
              <col style={{ width: 54 }} />
            </colgroup>
            <thead>
              <tr><th><span className="sr-only">{m.pickups.icon}</span></th><th>{m.pickups.item}</th><th className="num">{m.pickups.qty}</th><th className="num">{m.pickups.value}</th></tr>
            </thead>
            <tbody>
              {pickedUp.map((p) => (
                <tr key={p.itemId}>
                  <td><PatchView patch={p.patch} zoom={2} /></td>
                  <td className="name" title={p.name ?? undefined}>{p.name ?? <span className="unknown">{m.pickups.unnamed}</span>}</td>
                  <td className="num">+{p.count}</td>
                  <td className="num">{p.value === 0 && p.name === null ? "—" : formatWhole(p.value)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Scroller>
    </>
  );
}
