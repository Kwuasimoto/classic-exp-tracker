import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import { err, ok, type Result } from "./result";

const count = z.number().int().nonnegative();

export const AppErrorSchema = z.object({
  kind: z.enum(["storage", "invalid", "notFound"]),
  message: z.string(),
});
export type AppError = z.infer<typeof AppErrorSchema> | { kind: "ipc"; message: string };

export const ReaderStatusSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ok") }),
  z.object({ kind: z.literal("expUnreadable") }),
  z.object({ kind: z.literal("mapUnreadable") }),
  z.object({ kind: z.literal("starting") }),
  z.object({ kind: z.literal("screenNotSupported"), detail: z.string() }),
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
/** Saves what the live chart shows as a snapshot: its id, or null if there was nothing to save. */
export const saveSnapshot = (name: string, windowMinutes: 120 | 180) => call("save_snapshot", z.number().int().nullable(), { name, windowMinutes });
/** Ends the session and starts the live view afresh; saves nothing. */
export const resetSession = () => call("reset_session", z.null());
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

