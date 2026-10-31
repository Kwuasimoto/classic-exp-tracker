import { useEffect, useRef } from "react";

/** A patch's width in pixels (read::inventory::PATCH); its height follows from its length
 *  (12 rows from an icon, 8 from a keyboard quickslot key). */
const PATCH = 12;

/** The patch an item is recognised by, as the app saw it, drawn `zoom` times larger.
 *  Drawn from memory, never saved as an image. */
export function PatchView({ patch, zoom = 3 }: { patch: number[]; zoom?: number }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const rows = patch.length % (PATCH * 3) === 0 ? patch.length / (PATCH * 3) : 0;
  useEffect(() => {
    const ctx = canvas.current?.getContext("2d");
    if (!ctx || rows === 0) return;
    for (let i = 0; i < PATCH * rows; i++) {
      ctx.fillStyle = `rgb(${patch[i * 3]}, ${patch[i * 3 + 1]}, ${patch[i * 3 + 2]})`;
      ctx.fillRect((i % PATCH) * zoom, Math.floor(i / PATCH) * zoom, zoom, zoom);
    }
  }, [patch, rows, zoom]);
  return <canvas ref={canvas} className="patch" width={PATCH * zoom} height={rows * zoom} style={{ width: PATCH * zoom, height: rows * zoom }} aria-hidden="true" />;
}
