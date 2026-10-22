import { useCallback, useEffect, useRef, useState } from "react";
import {
  assignGlyph,
  dismissGlyph,
  exportGlyphs,
  forgetGlyph,
  getGlyphs,
  importGlyphs,
  type AppError,
  type GlyphKind,
  type Glyphs,
  type Tick,
} from "../lib/ipc";

type Section = "unknown" | "learned";
/** Ink levels of glyph art, as in read/exp.rs: ' ' = 0/9 … '@' = 9/9. */
const ART_LEVELS = " .:-=+*#%@";
const KINDS: readonly GlyphKind[] = ["exp", "level"];
const KIND_LABEL: Record<GlyphKind, string> = { exp: "EXP", level: "Level" };
const ALPHABET: Record<GlyphKind, string> = { exp: "0123456789,.[]%", level: "0123456789" };

/** A glyph drawn from its ASCII art, one cell per pixel. It's the stored shape, not a screenshot. */
function GlyphShape({ art, scale }: { art: string[]; scale: number }) {
  const width = art[0]?.length ?? 0;
  return (
    <div className="glyph" style={{ gridTemplateColumns: `repeat(${width}, ${scale}px)`, gridAutoRows: `${scale}px` }} aria-hidden="true">
      {art.flatMap((row, y) => [...row].map((c, x) => <span key={`${y}-${x}`} style={{ opacity: Math.max(ART_LEVELS.indexOf(c), 0) / 9 }} />))}
    </div>
  );
}

/** Characters the app reads, one library per resolution (the one in use is shown first):
 *  shapes it couldn't read wait under Unknown for the user to name; everything it knows
 *  is under Learned. Libraries can be exported to a file and imported again. */
export function GlyphsView({ tick }: { tick: Tick | null }) {
  const [glyphs, setGlyphs] = useState<Glyphs | null>(null);
  // The resolution picked in the tabs; null follows the one in use.
  const [viewed, setViewed] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [section, setSection] = useState<Section>("unknown");
  const [typed, setTyped] = useState<Record<number, string>>({});
  const [forgetting, setForgetting] = useState<number | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  // Reloads go out every second and can finish out of order: keep only the newest.
  const latestRequest = useRef(0);

  const load = useCallback(async () => {
    const request = ++latestRequest.current;
    const res = await getGlyphs(viewed);
    if (request !== latestRequest.current) return;
    if (res.success) setGlyphs(res.data);
    else setError(res.error);
  }, [viewed]);

  // Unknown glyphs can turn up (or be learned) any second: reload with every tick.
  useEffect(() => {
    void load();
  }, [tick, load]);

  const done = async (res: { success: true } | { success: false; error: AppError }) => {
    if (!res.success) return setError(res.error);
    setError(null);
    await load();
  };

  const assign = async (id: number) => {
    const res = await assignGlyph(id, typed[id] ?? "");
    if (res.success) setTyped(({ [id]: _, ...rest }) => rest);
    await done(res);
  };

  const forget = async (id: number) => {
    setForgetting(null);
    await done(await forgetGlyph(id));
  };

  const exportLibrary = async (resolution: string) => {
    setMessage(null);
    const res = await exportGlyphs(resolution);
    if (res.success && res.data !== null) setMessage(`Exported to ${res.data}`);
    await done(res);
  };

  const importLibrary = async () => {
    setMessage(null);
    const res = await importGlyphs();
    if (res.success && res.data !== null) {
      const { resolution, added, total } = res.data;
      setMessage(`Imported ${added} of ${total} glyphs into ${resolution}${added < total ? " (the rest were already there)" : ""}.`);
      setViewed(resolution === glyphs?.inUse ? null : resolution);
    }
    await done(res);
  };

  const unknown = glyphs?.unknown ?? [];
  const learned = glyphs?.learned ?? [];
  const shown = glyphs?.resolution ?? null;
  const canExport = learned.some((g) => g.id !== null);
  return (
    <>
      <section className="panel">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 style={{ margin: 0 }}>Glyphs</h2>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn" disabled={!shown || !canExport} title="Save this resolution's learned glyphs to a file" onClick={() => shown && void exportLibrary(shown)}>
              Export
            </button>
            <button className="btn" title="Add the glyphs from an exported file" onClick={() => void importLibrary()}>Import</button>
          </div>
        </div>
        {glyphs && glyphs.resolutions.length > 0 && (
          <div className="segmented" role="group" aria-label="Resolution" style={{ marginTop: 12, width: "max-content", maxWidth: "100%", flexWrap: "wrap" }}>
            {glyphs.resolutions.map((r) => (
              <button key={r} className={r === shown ? "current" : ""} aria-pressed={r === shown} onClick={() => setViewed(r === glyphs.inUse ? null : r)}>
                {r}
                {r === glyphs.inUse ? " · in use" : ""}
              </button>
            ))}
          </div>
        )}
        <div className="row" style={{ marginTop: 12 }}>
          <div className="segmented" role="group" aria-label="Glyph list">
            {(["unknown", "learned"] as const).map((s) => (
              <button key={s} className={s === section ? "current" : ""} aria-pressed={s === section} onClick={() => setSection(s)}>
                {s === "unknown" ? `Unknown (${unknown.length})` : "Learned"}
              </button>
            ))}
          </div>
        </div>
        <p className="caption" style={{ marginBottom: 0 }}>
          The app reads your EXP and level by matching each character against the glyphs for your screen resolution, picked
          automatically. It learns new ones by itself while you play; a shape it can't place waits under Unknown for you to
          type which character it is. Glyphs are stored character shapes, never screenshots.
        </p>
        {message && <p className="caption" style={{ marginBottom: 0 }}>{message}</p>}
      </section>
      {error && (
        <p className="panel row error">
          <span style={{ flex: 1 }}>{error.message}</span>
          <button className="btn" onClick={() => setError(null)}>Dismiss</button>
        </p>
      )}

      {section === "unknown" && (
        <section className="panel">
          {shown === null ? (
            <p className="caption">Waiting for the first screen capture…</p>
          ) : unknown.length === 0 ? (
            <p className="caption">Nothing unrecognised. Shapes the app can't read on your EXP or level show up here.</p>
          ) : (
            <div className="glyph-list">
              {unknown.map((u) => (
                <div key={u.id} className="glyph-card">
                  <GlyphShape art={u.art} scale={5} />
                  <span className="label">{KIND_LABEL[u.kind]}</span>
                  <input
                    value={typed[u.id] ?? ""}
                    maxLength={1}
                    aria-label={`${KIND_LABEL[u.kind]} character`}
                    title={`One of ${ALPHABET[u.kind].split("").join(" ")}`}
                    style={{ width: 44, textAlign: "center", fontSize: 18 }}
                    onChange={(e) => setTyped((t) => ({ ...t, [u.id]: e.target.value }))}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void assign(u.id);
                    }}
                  />
                  <div className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
                    <button className="btn primary" disabled={!typed[u.id]} onClick={() => void assign(u.id)}>Save</button>
                    <button className="btn" title="Not a character of the readout" onClick={() => void dismissGlyph(u.id).then(done)}>Dismiss</button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      )}

      {section === "learned" &&
        KINDS.map((kind) => {
          const entries = learned
            .filter((g) => g.kind === kind)
            .sort((a, b) => ALPHABET[kind].indexOf(a.ch) - ALPHABET[kind].indexOf(b.ch));
          const missing = kind === "exp" ? glyphs?.missingExp : glyphs?.missingLevel;
          return (
            <section key={kind} className="panel">
              <div className="row" style={{ justifyContent: "space-between" }}>
                <h2 style={{ margin: 0 }}>{KIND_LABEL[kind]}</h2>
                <span className="caption">{missing ? `Missing: ${missing.split("").join(" ")}` : "Every character learned"}</span>
              </div>
              {entries.length === 0 ? (
                <p className="caption">None yet.</p>
              ) : (
                <div className="glyph-list" style={{ marginTop: 12 }}>
                  {entries.map((g, i) => (
                    <div key={g.id ?? `built-in-${i}`} className="glyph-card">
                      <GlyphShape art={g.art} scale={3} />
                      <span style={{ fontSize: 18, fontWeight: 900 }}>{g.ch}</span>
                      <span className="caption">{g.id === null ? "built in" : "learned"}</span>
                      {g.id !== null &&
                        (forgetting === g.id ? (
                          <div className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
                            <button className="btn danger" onClick={() => void forget(g.id as number)}>Forget</button>
                            <button className="btn" onClick={() => setForgetting(null)}>Keep</button>
                          </div>
                        ) : (
                          <button className="btn" onClick={() => setForgetting(g.id)}>Forget</button>
                        ))}
                    </div>
                  ))}
                </div>
              )}
            </section>
          );
        })}
    </>
  );
}
