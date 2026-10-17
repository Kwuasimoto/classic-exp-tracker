import { useRef, useState, type MouseEvent } from "react";
import { formatPct } from "../lib/format";
import { getCalibrationFrame, previewRegions, saveCalibration, type AppError, type CalibrationFrame, type Rect, type RegionPreview } from "../lib/ipc";

type Target = "exp" | "map" | "level";
const TARGETS: readonly Target[] = ["exp", "map", "level"];
const LABELS: Record<Target, string> = { exp: "EXP box", map: "map box", level: "level box" };
// Presets for 1920x1080 (measured 2026-10-15/16); adjust if they miss.
const DEFAULT_MAP: Rect = { x: 62, y: 36, w: 240, h: 44 };
const DEFAULT_LEVEL: Rect = { x: 457, y: 1045, w: 50, h: 21 };
const COLORS: Record<Target, string> = { exp: "#2f6fde", map: "#e0742f", level: "#2fa86f" };

export function CalibrateView() {
  const [frame, setFrame] = useState<CalibrationFrame | null>(null);
  const [rects, setRects] = useState<Record<Target, Rect | null>>({ exp: null, map: DEFAULT_MAP, level: DEFAULT_LEVEL });
  const [target, setTarget] = useState<Target>("exp");
  const [drag, setDrag] = useState<{ x: number; y: number } | null>(null);
  const [preview, setPreview] = useState<RegionPreview | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  const grab = async () => {
    setError(null);
    setPreview(null);
    const res = await getCalibrationFrame();
    if (!res.success) return setError(res.error);
    setFrame(res.data);
    setRects({
      exp: res.data.current?.exp ?? null,
      map: res.data.current?.map ?? DEFAULT_MAP,
      level: res.data.current?.level ?? DEFAULT_LEVEL,
    });
  };

  const toImage = (e: MouseEvent) => {
    const img = imgRef.current;
    if (!img || !frame) return null;
    const b = img.getBoundingClientRect();
    const clamp = (v: number, max: number) => Math.min(Math.max(Math.round(v), 0), max);
    return {
      x: clamp(((e.clientX - b.left) * frame.width) / b.width, frame.width),
      y: clamp(((e.clientY - b.top) * frame.height) / b.height, frame.height),
    };
  };

  const onMove = (e: MouseEvent) => {
    const p = toImage(e);
    if (!drag || !p) return;
    setRects((r) => ({ ...r, [target]: { x: Math.min(drag.x, p.x), y: Math.min(drag.y, p.y), w: Math.abs(p.x - drag.x), h: Math.abs(p.y - drag.y) } }));
  };

  const setField = (key: keyof Rect, value: string) => {
    const n = Number.parseInt(value, 10);
    const current = rects[target];
    if (!current || Number.isNaN(n) || n < 0) return;
    setRects((r) => ({ ...r, [target]: { ...current, [key]: n } }));
  };

  const boxes = rects.exp && rects.map ? { exp: rects.exp, map: rects.map, level: rects.level } : null;

  const runPreview = async () => {
    if (!boxes) return;
    const res = await previewRegions(boxes);
    if (res.success) setPreview(res.data);
    else setError(res.error);
  };

  const save = async () => {
    if (!boxes) return;
    const res = await saveCalibration(boxes);
    if (res.success) setMessage("Saved. Tracking uses these boxes from now on.");
    else setError(res.error);
  };

  const active = rects[target];
  return (
    <>
      <div className="card">
        <p className="status">
          Open MapleStory, then grab a frame. Draw the <b style={{ color: COLORS.exp }}>EXP box</b> tightly around the numbers after
          "EXP." (leave room on the right for longer numbers). Check that the <b style={{ color: COLORS.map }}>map box</b> covers the
          street and map name under the minimap title, and that the <b style={{ color: COLORS.level }}>level box</b> covers only
          the number after "LV." (not the "LV." itself). The app learns the level digits by itself while you play.
        </p>
        <div className="row">
          <button className="primary" onClick={() => void grab()}>Grab frame</button>
          {TARGETS.map((t) => (
            <button key={t} className={t === target ? "primary" : ""} onClick={() => setTarget(t)}>
              Edit {LABELS[t]}
            </button>
          ))}
          <button disabled={!boxes || !frame} onClick={() => void runPreview()}>Preview</button>
          <button className="primary" disabled={!boxes || !frame} onClick={() => void save()}>Save</button>
        </div>
        {active && (
          <div className="row" style={{ marginTop: 8 }}>
            {(["x", "y", "w", "h"] as const).map((k) => (
              <label key={k}>
                {k} <input type="number" min={0} value={active[k]} onChange={(e) => setField(k, e.target.value)} style={{ width: 80 }} />
              </label>
            ))}
          </div>
        )}
        {error && <p className="error">{error.message}</p>}
        {message && <p className="status">{message}</p>}
        {preview && (
          <p>
            EXP: {preview.exp.kind === "ok" ? `${preview.exp.value.exp} (${formatPct(preview.exp.value.pctBp)})` : `✗ ${preview.exp.reason}`}
            <br />
            Map: {preview.map.kind === "ok" ? preview.map.value : `✗ ${preview.map.reason}`}
            {preview.level && (
              <>
                <br />
                Level: {preview.level.kind === "ok" ? preview.level.value : `✗ ${preview.level.reason}`}
              </>
            )}
          </p>
        )}
      </div>
      {frame && (
        <div
          className="card"
          style={{ position: "relative", padding: 0, overflow: "hidden", cursor: "crosshair", userSelect: "none" }}
          onMouseDown={(e) => setDrag(toImage(e))}
          onMouseMove={onMove}
          onMouseUp={() => setDrag(null)}
          onMouseLeave={() => setDrag(null)}
        >
          <img ref={imgRef} src={frame.pngDataUrl} alt="Captured frame" draggable={false} style={{ width: "100%", display: "block" }} />
          {TARGETS.map((t) => {
            const r = rects[t];
            return (
              r && (
                <div
                  key={t}
                  style={{
                    position: "absolute",
                    left: `${(r.x / frame.width) * 100}%`,
                    top: `${(r.y / frame.height) * 100}%`,
                    width: `${(r.w / frame.width) * 100}%`,
                    height: `${(r.h / frame.height) * 100}%`,
                    outline: `2px solid ${COLORS[t]}`,
                    pointerEvents: "none",
                  }}
                />
              )
            );
          })}
        </div>
      )}
    </>
  );
}
