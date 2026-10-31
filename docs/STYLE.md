# UI style guide

The look: a docked tool, not a game menu. The title bar, rail and panels meet edge to edge on
one solid ground, split by 1 px hairlines, in the Nexon launcher's black, white and orange. Every value below is a CSS variable or class in
`src/App.css`; new UI uses those, never new one-off colours.

## Sources

- Nexon launcher: near-black ground, bold uppercase labels, white pill as the
  primary action, dark segmented toggles.
- maplestory.nexon.net stylesheet (read 2026-10-16): MapleStory orange `#f68d13`,
  green `#51bb86`, three-layer Material shadow, Titillium Web for headings.

## Tokens

| Token | Value | Use |
|---|---|---|
| `--ground` | `#111113` | Behind everything; panels have no fill of their own |
| `--chrome` | `#0b0b0c` | The title bar and the rail |
| `--inset` | `#26262a` | Segmented toggles, inputs, table hover |
| `--line` | `#2a2a2f` | Grid lines, table rules |
| `--text` | `#ffffff` | Values, headings |
| `--muted` | `#a0a0a6` | Labels, captions, axis ticks |
| `--accent` | `#f68d13` | EXP/hr: its label, the rate line, its axis |
| `--meso` | `#f5c542` | Profit/hr (Mesos page): its label and the rate line, as orange is for EXP/hr |
| `--ok` | `#51bb86` | "Reading OK" |
| `--warn` | `#f0a04b` | Reader problems, errors |
| `--bar` | `#66666e` | EXP-per-10-min bars (3.3:1 on the ground, the minimum for graphics is 3:1) |

Orange means "EXP/hr" and nothing else, so the eye can find the rate anywhere; meso gold
means "Profit/hr" the same way.

## Type

Titillium Web (bundled from `@fontsource/titillium-web`, open licence), tabular numbers everywhere.

- Labels (`.label`): 11 px, 700, uppercase, `letter-spacing: 0.12em`, `--muted`.
- Ledger headline (`.ledger-total`): 40 px, 900; an empty one is a grey "—" at 400.
- Page heading: 28 px, 700.
- Body and table text: 14 px, 400.

## Shape and depth

- Radius: panels are square; buttons, toggles, icon buttons and the level badge have 4 px corners.
  Only the small count tags on items (`?`, HP/MP, the to-name count) stay round.
- Panels: no fill, no blur, no shadow; they sit on `--ground` and are split by 1 px `--line`
  hairlines (the ledger's right edge, between stacked panels). 12×14 px inside.
- Shadow: only the toast floats, with the MapleStory shadow (`--shadow`: `0 7px 8px -4px`,
  `0 12px 17px 2px`, `0 5px 22px 4px` in black at 40 / 28 / 24 %).

## Accessibility

- Keyboard: everything clickable is a button or link (a History row's name is its button); a
  scroller that overflows takes focus. The focus ring is white, inside a control, and outside light
  ones (primary and selected buttons, item squares), where a ring inside would vanish.
- Every page has one heading (`h1`); icon-only buttons have an `aria-label`; the chart has a title
  and description, and its arrow keys read each bar.
- Contrast: text at least 4.5:1 (grey text is 6.4–7.3:1), graphics at least 3:1.
- Windows high contrast (`forced-colors`): controls get a border, the selected one the system
  highlight colours. Reduced motion: the toast doesn't slide. Ctrl +/− zooms (`zoomHotkeysEnabled`).
- Checked with axe-core (WCAG 2.2 AA) on every page; keep it at zero violations.

## Languages

- Every word the UI shows comes from `src/locales/`: `en.ts` is the source, and each language
  (`zh-Hans`, `ko`, `ja`, `es`, `pt-BR`, `th`, `vi`) matches it key for key, which TypeScript
  checks. Text with values in it is a function, so each language orders words its own way.
- Write each language natively, with its own MapleStory terms (经验/金币, 경험치/메소,
  経験値/メル, เมโส…), in the same plain, short style as the English. Map and item names, numbers
  (26,000) and the logs stay as they are.
- `lib/i18n.ts` follows Windows' language unless the user picked one (the rail's globe button);
  it sets `lang` on the page, which picks Windows' own font for Chinese, Japanese, Korean and Thai
  (nothing bundled) and Segoe UI for Vietnamese, whose letters Titillium Web lacks.
- New text: add it to `en.ts`, then to every other language; check compressed mode (330 px) in the
  longest languages (Spanish, Portuguese, Vietnamese).

## Components

- **Title bar** (`TitleBar.tsx`): the window has no native frame. A 36 px `--chrome` strip over a
  hairline, the app icon and name at a 20 px inset, 46 px wide window buttons; close turns `#c42b1c`
  on hover. The whole strip drags the window (`data-tauri-drag-region`).
- **App icon**: three bars growing from small to large, white, white, orange
  (`src-tauri/icons/app-icon.svg`; regenerate the set with `pnpm tauri icon src-tauri/icons/app-icon.svg`).
- **Rail** (`.rail`): a 48 px `--chrome` column, a hairline on its right, of 36 px icon buttons for
  the screens (tooltips name them); the current one is `--inset` with a 2 px orange left edge. The
  popout and compressed-mode buttons sit at its foot.
- **Reader status** (`ReaderStatus.tsx`): a dot and a few words ("Reading OK" in `--ok`, a problem
  in `--warn`) at the right of every page header, the full explanation on hover.
- **Buttons**: `.btn` (dark, white text); `.btn.primary` (white, black text) for the one main action on a screen.
- **Icon button** (`.icon-btn`): 28 px `--inset` button for an icon-only action, with an `aria-label`.
- **Danger button** (`.btn.danger`): `#c42b1c`, only as the confirming step of a delete.
- **Toast** (`.toast`): white box floating at the bottom centre, outside the layout; says what just happened and fades after 3 s. Actions don't ask for confirmation unless they delete something.
- **Segmented toggle** (`.segmented`): `--inset` track, the selected option white.
- **Level badge** (`.badge`): white 1.5 px outline, `LV. 41`.
- **Page**: a one-line header band (`.page-head`: a graphite gradient, `#1e1e22` to `#141416`,
  with an orange hairline under it at 45 %; a 20 px title cut with "…", its icon buttons, its
  controls and the reader status on the right, wrapping when narrow), then
  `.page-body` (Live, Mesos): the ledger beside a column of the chart and anything else. The
  body fills the window down to 480 px tall; below that the page scrolls.
- **Ledger** (`Ledger.tsx`): one full-height panel, 290 px wide, a hairline on its right. One or two headline rates side
  by side at 26 px, each in its colour (EXP/hr orange, Profit/hr meso gold, a figure that isn't
  a rate plain), with a caption; an amber line under them when a map lock leaves the player
  elsewhere. Then tight groups of 13 px lines under small headings (label left, a grey note
  after it, value right, no rules); money going out is `--warn`. A heading can carry a grey note
  and, at its right, a 24 px icon button (Live's "Totals": EXP gained and profit since their own
  reset, which asks first with a danger button). The pickups table fills the rest.
- **Active pots** (`ActivePots.tsx`, Live, meso tracking on): a ledger group above the pickups.
  Its heading names the quickslot layout being read (keyboard or controller, with an icon), or
  says "Quickslot not read" in `--warn`. Then one line per main pot: HP/MP, its icon, its name
  (an unnamed one in `--warn`), how many are on the quickslot, and a chevron. Clicking a line
  opens the pots on the quickslot under it as 36 px squares with their counts (the current one
  outlined white), and a None button.
- **Chat translator** (`TranslatorView.tsx`): two views, switched by a segmented toggle in the page
  header: **Translations** (`LiveTranslations.tsx`) and **Setup**. Translations is laid out like a
  chat, never with bubbles: a log filling the panel (flat, left-aligned lines) above a reply box (an
  English input, the language to translate into, Translate), the translation to copy into the game.
  Until reading is built, the log holds one note: nothing installed (a "Set up a language" button),
  off (what it costs to run, and Turn on), or on. The header shows On/Off and Turn on/off, which is
  disabled until a language is installed. It opens on
  Translations when a language is installed, on Setup when none is.
  Setup (`TranslatorSetup.tsx`): a row per language (name, state, buttons): a progress
  bar while downloading, `--ok` "Installed", `--warn` when a download stopped (Retry, Dismiss);
  Remove asks first. With nothing installed, an amber-edged note says so. Below, every file it can
  download: part, source and name, SHA-256, size and license. Compressed, rows stack.
- **Scroller** (`Scroller.tsx`): a list that can grow (pickups, item grid) scrolls with no
  scrollbar and never sideways; while more is below, its last line fades out.
- **Item grid** (Mesos page): items as squares on the game's inventory grey, under tabs that
  never wrap; about three rows show. An item that needs a name has a `--warn` border and a "?"
  badge (never colour alone); an ignored one is dashed and faded; the main HP/MP items carry a
  white HP/MP tag. A legend sits by the tabs (under them when narrow). A clicked item opens one
  editor row under the grid.
- **Table** (History): fixed columns; long names and maps are cut with "…" (the full text on
  hover); a row's actions are 30 px icon buttons, and confirming steps are small buttons. An
  automatic snapshot's second line says when it will be deleted, and its row has a Keep button.
- **Chart**: bars `--bar` on the left axis (per 10 min), the rate line on the right axis in its
  colour (on Live with meso tracking, a dashed meso-gold Profit/hr line on a second right axis).
  The legend keys above the plot are borderless buttons that show or hide their part, a hidden
  one faded and struck through (remembered in local storage); they wrap under each other when
  narrow, with the actions top right. As many time labels as fit, one per 90 px. With nothing
  in the window it says so.
- **Compressed / expanded** (rail button at the foot, remembered in local storage): compressed
  drops the charts and stacks the panels in one column, split by hairlines; the rail is 40 px.
  Width goes to the numbers, height to legibility: panels keep a 10×12 px inset, lines keep 13 px
  type on a 22 px rhythm, groups are 12 px apart, the title is 17 px and the headlines 24 px. The
  item tabs narrow to 10 px sides so all three fit. The window shrinks to the rail
  and the ledger column and keeps the expanded height (`lib/windowSize.ts`);
  expanding restores the size it had. Mesos stacks its items under the ledger, History drops the
  period column.
- **Popout** (pushpin rail button, reopens with the app if it was open; `src/popout`): a small
  always-on-top window over the game, left out of screen capture so it never hides what the
  reader reads. A "stopwatch" ring: the outer ring is the level (white done, orange one more hour
  at the current rate), the thin inner ring laps once per hour of grinding time, which counts up
  `H:MM:SS` in the middle. A hairline splits it from the rates: labels left (12 px), numbers
  right (EXP/h 22 px orange, Meso/h 17 px gold, meso tracking only), then a hairline and a footer,
  "Lv N in **6h 59m**" (13 px) and a circled Σ whose tooltip holds the totals. The panel is opaque and no text is under 11 px or dimmer than
  #d4d4d8, so it reads over a busy game. A reader problem greys the numbers, turns the dot amber
  and puts the short status in the footer. 96 px tall, as wide as its numbers; drag anywhere, ✕
  on hover.

