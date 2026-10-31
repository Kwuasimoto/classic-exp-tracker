import type { ReactNode } from "react";

/** One line of a ledger: a label (with an optional note after it) and its value. */
export interface LedgerLine {
  label: string;
  note?: string;
  value: string;
  /** "spent" shows money going out, in the warning colour. */
  tone?: "spent";
}

/** A headline rate at the top of the ledger. */
export interface Headline {
  title: string;
  /** Its colour: orange for EXP/hr, meso gold for Profit/hr (docs/STYLE.md); plain for a
   *  figure that isn't a rate. */
  tone: "exp" | "meso" | "plain";
  value: string;
  caption: string;
}

/** A group of lines under a small heading. */
export interface LedgerGroup {
  title: string;
  /** A grey note after the heading. */
  note?: string;
  /** A control at the right of the heading (the totals' reset). */
  action?: ReactNode;
  lines: LedgerLine[];
}

/** The ledger column (Live, Mesos): one or two headline rates side by side, tight groups of
 *  the lines they're made of, and whatever the page adds below (the pickups list). One
 *  panel, the page's full height. */
export function Ledger({
  headlines,
  warning,
  groups,
  children,
}: {
  headlines: Headline[];
  warning?: string | null;
  groups: LedgerGroup[];
  children?: ReactNode;
}) {
  return (
    <section className="panel ledger">
      <div className="ledger-headlines">
        {headlines.map((h) => (
          <div key={h.title} className="ledger-headline">
            <div className={h.tone === "exp" ? "label accent" : h.tone === "meso" ? "label meso" : "label"}>{h.title}</div>
            <div className={h.value === "—" ? "ledger-total empty" : "ledger-total"}>{h.value}</div>
            <div className="caption">{h.caption}</div>
          </div>
        ))}
      </div>
      {warning && <div className="caption warn">{warning}</div>}
      {groups.map((g) => (
        <div key={g.title} className="ledger-group">
          <div className="ledger-group-head">
            <span className="label">{g.title}</span>
            {g.note && <span className="caption">{g.note}</span>}
            {g.action && <span className="ledger-group-action">{g.action}</span>}
          </div>
          <LedgerLines lines={g.lines} />
        </div>
      ))}
      {children}
    </section>
  );
}

export function LedgerLines({ lines }: { lines: LedgerLine[] }) {
  return (
    <dl className="ledger-lines">
      {lines.map((l) => (
        <div key={l.label}>
          <dt>
            {l.label}
            {l.note && <span className="note">{l.note}</span>}
          </dt>
          <dd className={l.tone === "spent" ? "spent" : undefined}>{l.value}</dd>
        </div>
      ))}
    </dl>
  );
}
