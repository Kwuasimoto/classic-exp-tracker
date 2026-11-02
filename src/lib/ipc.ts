import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import { err, ok, type Result } from "./result";

const count = z.number().int().nonnegative();

export const AppErrorSchema = z.object({
  kind: z.enum(["storage", "invalid", "notFound", "window", "translator", "update"]),
  message: z.string(),
});
export type AppError = z.infer<typeof AppErrorSchema> | { kind: "ipc"; message: string };

export const ReaderStatusSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ok") }),
  z.object({ kind: z.literal("expUnreadable") }),
  z.object({ kind: z.literal("mapUnreadable") }),
  z.object({ kind: z.literal("gameNotFound") }),
  z.object({ kind: z.literal("starting") }),
  z.object({ kind: z.literal("screenNotSupported"), detail: z.string() }),
  z.object({ kind: z.literal("captureFailed"), detail: z.string() }),
  z.object({ kind: z.literal("storageFailed"), detail: z.string() }),
  z.object({ kind: z.literal("notifyFailed"), detail: z.string() }),
]);
export type ReaderStatus = z.infer<typeof ReaderStatusSchema>;

export const LootSchema = z.object({ meso: count, etcValue: count, potionCost: count });
export type Loot = z.infer<typeof LootSchema>;

export const CornerSchema = z.enum(["topRight", "bottomRight", "bottomLeft"]);
export type Corner = z.infer<typeof CornerSchema>;

export const PickedUpSchema = z.object({
  itemId: z.number().int(),
  name: z.string().nullable(),
  count,
  value: count,
  patch: z.array(z.number().int().min(0).max(255)),
});
export type PickedUp = z.infer<typeof PickedUpSchema>;

export const MainPotionSchema = z.object({ itemId: z.number().int(), name: z.string().nullable() });
export type MainPotion = z.infer<typeof MainPotionSchema>;
export type PotionSlot = "hp" | "mp";

/** A Use item on the quickslot and how many are there; `patch` as on screen. */
export const QuickslotPotSchema = z.object({
  itemId: z.number().int(),
  name: z.string().nullable(),
  count,
  patch: z.array(z.number().int().min(0).max(255)),
});
export type QuickslotPot = z.infer<typeof QuickslotPotSchema>;

export const LootStatusSchema = z.object({
  enabled: z.boolean(),
  corner: CornerSchema,
  found: z.boolean(),
  inventory: z.boolean(),
  mesoTotal: count.nullable(),
  etcTab: z.boolean(),
  quickslot: z.boolean(),
  unnamedItems: count,
  pickedUp: z.array(PickedUpSchema),
  mainHp: MainPotionSchema.nullable(),
  mainMp: MainPotionSchema.nullable(),
  /** The quickslot layout read this second; null when it didn't read. */
  quickslotLayout: z.enum(["controller", "keyboard"]).nullable(),
  /** The Use items on the quickslot when it last read, in slot order. */
  quickslotPots: z.array(QuickslotPotSchema),
  /** When the inventory was last seen with everything made counted (unix seconds): the loot
   *  rates are held at that moment while it's closed. */
  seenAt: z.number().int().nullable(),
});
export type LootStatus = z.infer<typeof LootStatusSchema>;

export const TickSchema = z.object({
  status: ReaderStatusSchema,
  mapId: z.number().int().nullable(),
  mapName: z.string().nullable(),
  shopping: z.boolean(),
  lockedMap: z.number().int().nullable(),
  lockedName: z.string().nullable(),
  /** EXP into the level; worked out from the percent (expExact false) past 1,000,000, where
   *  the game shows only the percent. */
  exp: count.nullable(),
  expExact: z.boolean(),
  pctBp: count.nullable(),
  totalEst: count.nullable(),
  level: count.nullable(),
  loot: LootStatusSchema,
  /** Training time since the live view started (seconds), across maps: the popout's clock. */
  grindSecs: count,
  /** EXP and mesos made on every map since the user last reset the totals; `since` is the
   *  first minute counted (unix seconds), null until one is. */
  totals: z.object({ exp: count, loot: LootSchema, since: z.number().int().nullable() }),
});
export type Tick = z.infer<typeof TickSchema>;

const perHour = z.number().nonnegative();
export const BarSchema = z.object({
  start: z.number().int(),
  expGained: count,
  ratePerHour: perHour.nullable(),
  loot: LootSchema,
  lootPerHour: z.object({ meso: perHour, etcValue: perHour, potionCost: perHour }).nullable(),
});
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
  loot: LootSchema,
  /** Saved automatically at the end of a grind; deleted 30 days after `createdAt` unless kept. */
  auto: z.boolean(),
});
export type Snapshot = z.infer<typeof SnapshotSchema>;

export const ItemKindSchema = z.enum(["etc", "use"]);
export type ItemKind = z.infer<typeof ItemKindSchema>;

/** An item the app has seen on screen; `patch` is its RGB pixels, PATCH (12) wide, row by row. */
export const ItemSchema = z.object({
  id: z.number().int(),
  kind: ItemKindSchema,
  name: z.string().nullable(),
  ignored: z.boolean(),
  price: count.nullable(),
  patch: z.array(z.number().int().min(0).max(255)),
});
export type Item = z.infer<typeof ItemSchema>;

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

/** The live chart's windows, as the backend accepts them (rates::WINDOWS_MINUTES). */
export const WINDOWS_MINUTES = [120, 180] as const;
export type WindowMinutes = (typeof WINDOWS_MINUTES)[number];

export const getLiveSnapshot = () => call("get_live_snapshot", TickSchema);
export const getLiveChart = (windowMinutes: WindowMinutes) => call("get_live_chart", z.array(BarSchema), { windowMinutes });
/** Saves what the live chart shows as a snapshot: its id, or null if there was nothing to save. */
export const saveSnapshot = (name: string, windowMinutes: WindowMinutes) => call("save_snapshot", z.number().int().nullable(), { name, windowMinutes });
/** Counts only this map in the live view (chart, rates, snapshots) until unlocked. */
export const lockMap = (mapId: number) => call("lock_map", z.null(), { mapId });
export const unlockMap = () => call("unlock_map", z.null());
/** Meso tracking on or off (off, the inventory and quickslot aren't read at all). */
export const setLootEnabled = (on: boolean) => call("set_loot_enabled", z.null(), { on });
/** The corner the inventory is kept in; the app looks for it there. */
export const setInventoryCorner = (corner: Corner) => call("set_inventory_corner", z.null(), { corner });
/** Every item seen, unnamed ones first. */
export const getItems = () => call("get_items", z.array(ItemSchema));
/** The item list's names of one kind, to name an item from. */
export const itemNames = (kind: ItemKind) => call("item_names", z.array(z.string()), { kind });
/** Chooses the main HP or MP potion (null clears it): only those two count as potions used. */
export const setMainPotion = (slot: PotionSlot, itemId: number | null) => call("set_main_potion", z.null(), { slot, itemId });
/** Stops tracking an item (or tracks it again): ignored, it's recognised but never counted. */
export const ignoreItem = (itemId: number, ignored: boolean) => call("ignore_item", z.null(), { itemId, ignored });
/** Forgets an item learned by mistake; if it's really there, it's learned again. */
export const deleteItem = (itemId: number) => call("delete_item", z.null(), { itemId });
/** Names an item from the item list; it counts at the list's price from then on. */
export const nameItem = (itemId: number, name: string) => call("name_item", z.null(), { itemId, name });
/** Names a map; OCR never renames it after this. */
export const renameMap = (mapId: number, name: string) => call("rename_map", z.null(), { mapId, name });
/** Opens or closes the popout: the rates in a small window pinned above the game. */
/** The two windows pinned above the game (commands::Popout). */
export type Popout = "rates" | "chat";
export const setPopout = (popout: Popout, open: boolean) => call("set_popout", z.null(), { popout, open });
/** Ends the session and starts the live view afresh; saves nothing. */
export const resetSession = () => call("reset_session", z.null());
export const resetTotals = () => call("reset_totals", z.null());
export const getSnapshots = () => call("get_snapshots", z.array(SnapshotSchema));
export const getSnapshotChart = (snapshotId: number) => call("get_snapshot_chart", z.array(BarSchema), { snapshotId });
export const renameSnapshot = (snapshotId: number, name: string) => call("rename_snapshot", z.null(), { snapshotId, name });
export const deleteSnapshot = (snapshotId: number) => call("delete_snapshot", z.null(), { snapshotId });
export const keepSnapshot = (snapshotId: number) => call("keep_snapshot", z.null(), { snapshotId });

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


/** A chat line's English translation, kept with the line so every window shows the same one. */
export const EnglishSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("busy") }),
  z.object({ state: z.literal("done"), text: z.string(), model: z.string() }),
  z.object({ state: z.literal("failed"), error: z.string() }),
]);
export type English = z.infer<typeof EnglishSchema>;

/** A chat message as read (chat_reader.rs): the sender is empty on notices and crate
 *  announcements; confidence is the reader's lowest for any character. */
export const ChatLineSchema = z.object({
  id: z.number().int(),
  t: z.number().int(),
  /** The game's own "hh:mm", when the player has its chat timestamps on. */
  time: z.string().nullable(),
  kind: z.string(),
  channel: z.number().int().nullable(),
  sender: z.string(),
  text: z.string(),
  confidence: z.number(),
  english: EnglishSchema.nullable(),
});
export type ChatLine = z.infer<typeof ChatLineSchema>;

export const chatLines = () => call("chat_lines", z.array(ChatLineSchema));
export const onChatLine = (cb: (line: Result<ChatLine, AppError>) => void) =>
  subscribe("chat-line", (payload) => {
    const parsed = ChatLineSchema.safeParse(payload);
    cb(parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected chat line: ${parsed.error.message}` }));
  });
const EnglishChangeSchema = z.object({ id: z.number().int(), english: EnglishSchema });
/** A line's translation started, was done or failed, in whichever window it was asked for. */
export const onChatEnglish = (cb: (change: Result<z.infer<typeof EnglishChangeSchema>, AppError>) => void) =>
  subscribe("chat-english", (payload) => {
    const parsed = EnglishChangeSchema.safeParse(payload);
    cb(parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected chat translation: ${parsed.error.message}` }));
  });

/** The chat translator (translator.rs): on or off, and its chat reader's download (the
 *  one running, or the last one if it failed, with why). */
export const TranslatorStatusSchema = z.object({
  /** On (the user turned it on, and the chat reader is downloaded); off, nothing of it runs. */
  enabled: z.boolean(),
  downloaded: z.boolean(),
  /** What downloading would fetch now. */
  downloadBytes: count,
  job: z.object({ doneBytes: count, totalBytes: count, error: z.string().nullable() }).nullable(),
  dir: z.string(),
  /** Each download, its pinned files and its pages (translator.rs PARTS). */
  parts: z.array(
    z.object({
      id: z.string(),
      homepage: z.string(),
      sourcePage: z.string(),
      installed: z.boolean(),
      files: z.array(
        z.object({
          name: z.string(),
          url: z.string(),
          size: count,
          sha256: z.string(),
          license: z.string(),
          /** The one file kept from a zip download, checked on its own. */
          extract: z.object({ path: z.string(), size: count, sha256: z.string() }).nullable(),
        }),
      ),
    }),
  ),
});
export type TranslatorStatus = z.infer<typeof TranslatorStatusSchema>;

export const translatorStatus = () => call("translator_status", TranslatorStatusSchema);
/** Starts the chat reader's download; only ever from its Download button. */
export const downloadReader = () => call("download_reader", z.null());
export const cancelInstall = () => call("cancel_install", z.null());
export const dismissInstallError = () => call("dismiss_install_error", z.null());
/** Opens a chat reader part's page (or its files' page) in the browser; only pages pinned in translator.rs. */
export const openPartPage = (id: string, source: boolean) => call("open_part_page", z.null(), { id, source });
/** Deletes the chat reader's files; the translator turns off. */
export const removeReader = () => call("remove_reader", z.null());
/** On needs the chat reader downloaded. */
export const setTranslatorEnabled = (on: boolean) => call("set_translator_enabled", z.null(), { on });
/** What replies can be translated into (gemini.rs LANGUAGES). */
export const REPLY_LANGUAGES = ["zh-hans", "zh-pinyin", "ko", "ja", "es", "pt-br", "th"] as const;

/** Whether a Gemini API key is saved (the key stays in Windows Credential Manager). */
export const translatorKeySaved = () => call("translator_key_saved", z.boolean());
/** Saves the key once Google answered a request made with it; resolves to the model that answered. */
export const saveTranslatorKey = (key: string) => call("save_translator_key", z.string(), { key });
export const removeTranslatorKey = () => call("remove_translator_key", z.null());
/** The user's reply, translated by Gemini into `language` (one of REPLY_LANGUAGES). */
export const translateReply = (text: string, language: string) =>
  call("translate_reply", z.object({ text: z.string(), model: z.string() }), { text, language });

/** Translate chat line `id` into English, whatever its language (gemini.rs); the translation
 *  arrives in every window through onChatEnglish. */
export const translateLine = (id: number) => call("translate_line", z.null(), { id });

/** Updates (updater.rs): checked 10 s after launch and hourly while `autoCheck`; `ready` is a
 *  newer version, downloaded and its signature checked, that installs on Restart. */
export const UpdateStatusSchema = z.object({
  autoCheck: z.boolean(),
  checking: z.boolean(),
  checkedAt: z.number().int().nullable(),
  available: z.string().nullable(),
  /** What changed in each version after the running one, up to `available`, newest first
   *  (updater.rs, from patch-notes.json); the notes are in English. */
  notes: z.array(
    z.object({
      version: z.string(),
      date: z.string(),
      new: z.array(z.string()),
      convenience: z.array(z.string()),
      technical: z.array(z.string()),
      fixes: z.array(z.string()),
    }),
  ),
  installing: z.boolean(),
  error: z.string().nullable(),
});
export type UpdateStatus = z.infer<typeof UpdateStatusSchema>;

export const updateStatus = () => call("update_status", UpdateStatusSchema);
export const checkForUpdates = () => call("check_for_updates", UpdateStatusSchema);
/** Downloads and checks the new version; then the app exits, the installer runs, and it starts. */
export const installUpdate = () => call("install_update", z.null());
export const setUpdateChecks = (on: boolean) => call("set_update_checks", z.null(), { on });
export const onUpdateStatus = (cb: (status: Result<UpdateStatus, AppError>) => void) =>
  subscribe("update-status", (payload) => {
    const parsed = UpdateStatusSchema.safeParse(payload);
    cb(parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected update status: ${parsed.error.message}` }));
  });

/** The popout closed (its own ✕, or the app closing it). */
export const onPopoutClosed = (cb: (popout: Popout) => void) =>
  subscribe("popout-closed", (payload) => {
    if (payload === "rates" || payload === "chat") cb(payload);
  });
