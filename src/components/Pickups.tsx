import { formatWhole } from "../lib/format";
import type { PickedUp } from "../lib/ipc";
import { PatchView } from "./PatchView";
import { Scroller } from "./Scroller";

/** The Etc picked up this session, one line per item, most valuable first (Live, Mesos). */
export function Pickups({ pickedUp }: { pickedUp: PickedUp[] }) {
  return (
    <>
      <div className="label ledger-heading">Picked up</div>
      <Scroller className="pickups">
        {pickedUp.length === 0 ? (
          <p className="caption">Etc you pick up this session shows here, most valuable first.</p>
        ) : (
          <table>
            <colgroup>
              <col style={{ width: 28 }} />
              <col />
              <col style={{ width: 44 }} />
              <col style={{ width: 54 }} />
            </colgroup>
            <thead>
              <tr><th><span className="sr-only">Icon</span></th><th>Item</th><th className="num">Qty</th><th className="num">Value</th></tr>
            </thead>
            <tbody>
              {pickedUp.map((p) => (
                <tr key={p.itemId}>
                  <td><PatchView patch={p.patch} zoom={2} /></td>
                  <td className="name" title={p.name ?? undefined}>{p.name ?? <span className="unknown">? Unnamed</span>}</td>
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
