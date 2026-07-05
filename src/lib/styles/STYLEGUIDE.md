# Veydan Browser — UI style guide

Single source of truth for styling. **Before adding CSS, check whether a token or
primitive already exists.** New ad-hoc colours/sizes/components are how the UI
drifted in the first place.

## Layers

| File | Role |
|------|------|
| `src/lib/styles/tokens.css` | Design tokens — colours (dark/light), spacing, type, radius, z-index, sizes, motion. The **only** place raw values live. |
| `src/lib/styles/base.css` | Global reset + reusable primitives (`.btn`, `.badge`, `.page`, `.card`, `.tab-bar`, `.empty-state`, `.spinner`, …). |
| `src/lib/components/ui/Dialog.svelte` | Centered modal shell. |
| `src/lib/components/ui/Drawer.svelte` | Right/left side-panel shell. |

Both CSS files are imported once at the top of `src/routes/+layout.svelte`.

## Rules

1. **Colour** → always `var(--…)`. Never a raw hex/rgba in a component (except pure
   `#fff` on a coloured button). If you need a new colour, add it to `tokens.css`
   in **both** themes.
2. **Spacing / radius / font-size** → use `--sp-*`, `--radius*`, `--fs-*`. Don't
   invent `0.42rem`.
3. **z-index** → use `--z-*` (`--z-drawer`, `--z-modal`, `--z-toast`, …). Never a
   bare number.
4. **Never invent a token name.** Only names defined in `tokens.css` exist. A
   `var(--made-up)` with no fallback renders as transparent/inherited. Run the
   guard (below) — it fails on undefined references.
5. **Reuse primitives.** Need a badge/tab/empty-state/card? Use the global class.
   Only add scoped CSS for a genuine one-off delta.

## Tokens cheat-sheet

- Surfaces: `--bg` < `--bg-2` < `--surface` < `--surface-2`; borders `--border` / `--border-2`.
- Text: `--text` (primary) / `--text-2` (secondary) / `--text-3` (tertiary).
- Semantic: `--accent*`, `--success*`, `--danger*`, `--warn-*`.
- Spacing (4px grid): `--sp-1`=4 … `--sp-6`=24, `--sp-8`=32.
- Type: `--fs-xs` 0.72 · `--fs-sm` 0.8 · `--fs-base` 0.875 · `--fs-md` 1 · `--fs-lg` 1.15 · `--fs-xl` 1.4rem.
- Radius: `--radius-xs` 4 · `--radius-sm` 6 · `--radius` 10 · `--radius-pill` 999.
- Size: `--topbar-h` 44 · `--dock-h`/`--bar-h` 36 · `--drawer-w` 380 · `--drawer-w-lg` 520 · `--dialog-w` 440.

## Primitives

`.page` (+`--page-max` override) · `.page-header`(+`.spacer`) · `.card`/`.card-title`
· `.section`/`.section-label` · `.muted` · `.btn`(+`-primary/-ghost/-success/-danger/-sm`)
· `.icon-btn`(+`.success/.danger`) · `.badge`(+`-accent/-ok/-danger/-warn`) · `.chip`(+`.active`)
· `.tab-bar`/`.tab`(+`.active`)/`.tab-count` · `.empty-state` · `.loading`/`.spinner`/`.spin`
· `.form-group`/`.form-row` · `.error-msg`.

## Modals & drawers

Use the shells instead of hand-rolling an overlay (that gave us z-index 20→1000 and
inconsistent widths/backdrops):

```svelte
<Drawer open title="Edit proxy" {onclose}>
  <form>…</form>            <!-- body: scrolls, gutter-stable, min-height:0 baked in -->
</Drawer>

<Dialog open title="Import" {onclose}>
  …body…
  {#snippet footer()}<button class="btn btn-primary">Save</button>{/snippet}
</Dialog>
```

Both handle: portal to `<body>`, backdrop, Escape/backdrop close, tokenized
z-index (`--z-drawer` / `--z-modal`), `--drawer-w`/`--dialog-w` width, slide/scale
animation with `prefers-reduced-motion`.

## Guardrail

```
bash scripts/check-styles.sh
```

Fails on any undefined token reference; reports raw-hex / raw-font-size drift counts.
Run it in CI / before committing style changes.

## Migration status — FINALIZED

Product UI is fully on the system:
- **All 10 overlays** use the shells: `<Drawer>` (ProxyPanel, CreateProfilePanel,
  EditProfilePanel, ProfileSidePanel, RawDataPanel, PasswordGenerator, TotpGenerator)
  and `<Dialog>` (TotpAddModal, ImportProfileModal, ExportProfileModal). Fixed
  toolbars/tabs go in the Drawer `subheader` snippet; header controls in `actions`;
  bottom buttons in `footer`.
- **All routes + components** reuse the primitives (`.page`/`.page-header`/`.card`/
  `.badge`/`.chip`/`.empty-state`/`.tab-bar`/`.loading`/`.muted`), local duplicates deleted.
- **Values tokenized**: font-size snapped to `--fs-*` (raw count 358 → 9, the 9 being
  inspector dev-tooling + one inline style); exact-grid spacing → `--sp-*`; palette
  hex → colour tokens. Guard passes (every `var()` resolves).

Deliberately left local (correct, not debt): per-item **dynamic-colour** tag chips /
status dots (color comes from data via inline style — a fixed-colour `.badge` can't
express them); bespoke tight **icon buttons** in notes/editor toolbars; the inspector
dev-tool overlay (`src/lib/inspector/*`, not product surface); off-grid spacing and
non-palette semantic hex that are intentional.

When adding UI: use a shell + primitives + tokens. Run `scripts/check-styles.sh`.
