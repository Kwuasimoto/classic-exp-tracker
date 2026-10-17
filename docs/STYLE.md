# UI style guide

The look: dark panels floating over a MapleStory backdrop, in the Nexon launcher's
black, white and orange. Every value below is a CSS variable or class in
`src/App.css`; new UI uses those, never new one-off colours.

## Sources

- Nexon launcher: near-black ground, bold uppercase labels, white pill as the
  primary action, dark segmented toggles.
- maplestory.nexon.net stylesheet (read 2026-10-16): MapleStory orange `#f68d13`,
  green `#51bb86`, 10 px card radius, pill buttons, three-layer Material shadow,
  10 px backdrop blur, Titillium Web for headings.

## Tokens

| Token | Value | Use |
|---|---|---|
| `--ground` | `#0a0a0b` | Behind everything when no backdrop |
| `--panel` | `rgba(20, 20, 22, 0.86)` | Every panel (`.panel`) |
| `--nav` | `rgba(0, 0, 0, 0.84)` | The side navigation |
| `--inset` | `#26262a` | Segmented toggles, inputs, table hover |
| `--line` | `#2a2a2f` | Grid lines, table rules |
| `--text` | `#ffffff` | Values, headings |
| `--muted` | `#a0a0a6` | Labels, captions, axis ticks |
| `--accent` | `#f68d13` | EXP/hr: its label, the rate line, its axis |
| `--ok` | `#51bb86` | "Reading OK" |
| `--warn` | `#f0a04b` | Reader problems, errors |
| `--bar` | `#55555d` | EXP-per-10-min bars |

Orange means "EXP/hr" and nothing else, so the eye can find the rate anywhere.

## Type

Titillium Web (bundled from `@fontsource/titillium-web`, open licence), tabular numbers everywhere.

- Labels (`.label`): 11 px, 700, uppercase, `letter-spacing: 0.12em`, `--muted`.
- Values (`.value`): 30 px, 900.
- Page heading: 28 px, 700.
- Body and table text: 14 px, 400.

## Shape and depth

- Radius: 10 px panels, 999 px (pill) for buttons, toggles and the level badge.
- Panels: `--panel` + 10 px backdrop blur + the MapleStory shadow
  (`--shadow`: `0 7px 8px -4px`, `0 12px 17px 2px`, `0 5px 22px 4px` in black at 40 / 28 / 24 %).
- Spacing: 20 px around and between the nav and content, 12–14 px between panels.

## Components

- **Side nav**: uppercase pill links; the current one is a white pill with black text.
- **Buttons**: `.btn` (dark pill, white text); `.btn.primary` (white pill, black text) for the one main action on a screen.
- **Segmented toggle** (`.segmented`): `--inset` track, white pill for the selected option.
- **Level badge** (`.badge`): white 1.5 px outlined pill, `LV. 41`.
- **Stat tile**: `.panel` with `.label`, `.value` and an optional caption.
- **Chart**: bars `--bar` on the left axis (EXP per 10 min), orange line on the right axis (EXP/hr), legend above the plot, fills its panel's height.

## Backdrops

`public/maplestory-background-{1,2,3}.jpg` (Ellinia, character party, Kerning City), or none.
The viewer picks one in the side nav; the choice is kept in the browser's local storage.
