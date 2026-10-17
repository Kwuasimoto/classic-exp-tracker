import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import { err, ok, type Result } from "./result";

const count = z.number().int().nonnegative();

export const AppErrorSchema = z.object({
  kind: z.enum(["storage", "capture", "invalid", "notFound"]),
  message: z.string(),
});
export type AppError = z.infer<typeof AppErrorSchema> | { kind: "ipc"; message: string };

export const ReaderStatusSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ok") }),
  z.object({ kind: z.literal("expUnreadable") }),
  z.object({ kind: z.literal("mapUnreadable") }),
  z.object({ kind: z.literal("notCalibrated") }),
  z.object({ kind: z.literal("captureFailed"), detail: z.string() }),
  z.object({ kind: z.literal("storageFailed"), detail: z.string() }),
  z.object({ kind: z.literal("notifyFailed"), detail: z.string() }),
]);
export type ReaderStatus = z.infer<typeof ReaderStatusSchema>;

export const TickSchema = z.object({
  status: ReaderStatusSchema,
  mapName: z.string().nullable(),
  exp: count.nullable(),
  pctBp: count.nullable(),
  totalEst: count.nullable(),
  level: count.nullable(),
});
export type Tick = z.infer<typeof TickSchema>;

export const BarSchema = z.object({ start: z.number().int(), expGained: count, ratePerHour: z.number().nonnegative().nullable() });
export type Bar = z.infer<typeof BarSchema>;

export const MapSummarySchema = z.object({
  mapId: z.number().int(),
  name: z.string(),
  sessions: count,
  trainingSecs: count,
  expGained: count,
  avgRatePerHour: z.number().nullable(),
  bestHourRate: z.number().nullable(),
});
export type MapSummary = z.infer<typeof MapSummarySchema>;

export const SessionSchema = z.object({
  id: z.number().int(),
  mapId: z.number().int(),
  mapName: z.string(),
  startedAt: z.number().int(),
  endedAt: z.number().int().nullable(),
  expGained: count,
  trainingSecs: count,
});
export type Session = z.infer<typeof SessionSchema>;

export const SnapshotSchema = z.object({
  id: z.number().int(),
  name: z.string(),
  createdAt: z.number().int(),
  mapName: z.string().nullable(),
  level: count.nullable(),
  from: z.number().int().nullable(),
  to: z.number().int().nullable(),
  expGained: count,
  trainingSecs: count,
});
export type Snapshot = z.infer<typeof SnapshotSchema>;

export const RectSchema = z.object({ x: count, y: count, w: count, h: count });
export type Rect = z.infer<typeof RectSchema>;
export const CalibrationSchema = z.object({ exp: RectSchema, map: RectSchema, level: RectSchema.nullable() });
export type Calibration = z.infer<typeof CalibrationSchema>;

export const CalibrationFrameSchema = z.object({
  width: z.number().int().positive(),
  height: z.number().int().positive(),
  pngDataUrl: z.string().startsWith("data:image/png;base64,"),
  current: CalibrationSchema.nullable(),
});
export type CalibrationFrame = z.infer<typeof CalibrationFrameSchema>;

const previewOf = <T extends z.ZodType>(value: T) =>
  z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("ok"), value }),
    z.object({ kind: z.literal("failed"), reason: z.string() }),
  ]);
export const RegionPreviewSchema = z.object({
  exp: previewOf(z.object({ exp: count, pctBp: count })),
  map: previewOf(z.string()),
  level: previewOf(count).nullable(),
});
export type RegionPreview = z.infer<typeof RegionPreviewSchema>;

/** Tauri rejects command promises with the serialized AppError; anything else is an IPC failure. */
export function toAppError(e: unknown): AppError {
  const parsed = AppErrorSchema.safeParse(e);
  return parsed.success ? parsed.data : { kind: "ipc", message: String(e) };
}

async function call<S extends z.ZodType>(cmd: string, schema: S, args?: Record<string, unknown>): Promise<Result<z.infer<S>, AppError>> {
  let raw: unknown;
  try {
    raw = await invoke(cmd, args);
  } catch (e) {
    return err(toAppError(e));
  }
  const parsed = schema.safeParse(raw);
  return parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected ${cmd} payload: ${parsed.error.message}` });
}

export const getLiveSnapshot = () => call("get_live_snapshot", TickSchema);
export const getLiveChart = (windowMinutes: 120 | 180) => call("get_live_chart", z.array(BarSchema), { windowMinutes });
export const getMapSummaries = () => call("get_map_summaries", z.array(MapSummarySchema));
export const renameMap = (mapId: number, name: string) => call("rename_map", z.null(), { mapId, name });
export const getSessions = (mapId: number | null) => call("get_sessions", z.array(SessionSchema), { mapId });
export const getSessionChart = (sessionId: number) => call("get_session_chart", z.array(BarSchema), { sessionId });
export const getCalibrationFrame = () => call("get_calibration_frame", CalibrationFrameSchema);
export const previewRegions = (c: Calibration) => call("preview_regions", RegionPreviewSchema, c);
export const saveCalibration = (c: Calibration) => call("save_calibration", z.null(), c);
export const forgetLevelDigits = () => call("forget_level_digits", z.null());
/** Saves the last 2 h as a snapshot (id, or null if there was nothing) and starts a fresh session. */
export const resetSession = (name: string) => call("reset_session", z.number().int().nullable(), { name });
export const getSnapshots = () => call("get_snapshots", z.array(SnapshotSchema));
export const getSnapshotChart = (snapshotId: number) => call("get_snapshot_chart", z.array(BarSchema), { snapshotId });
export const renameSnapshot = (snapshotId: number, name: string) => call("rename_snapshot", z.null(), { snapshotId, name });
export const deleteSnapshot = (snapshotId: number) => call("delete_snapshot", z.null(), { snapshotId });

async function subscribe(event: string, handler: (payload: unknown) => void): Promise<Result<UnlistenFn, AppError>> {
  try {
    return ok(await listen<unknown>(event, (e) => handler(e.payload)));
  } catch (e) {
    return err(toAppError(e));
  }
}

export const onTick = (cb: (tick: Result<Tick, AppError>) => void) =>
  subscribe("tracker-tick", (payload) => {
    const parsed = TickSchema.safeParse(payload);
    cb(parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected tick: ${parsed.error.message}` }));
  });

