# Verkkokyylä Design System

Derived from Claude Design mockups. The signature is a quiet command surface: warm-paper light theme, deep-green dark theme, dark-green sidebar in both, with small emerald/amber/brick accents carrying meaning.

## 1. Atmosphere & Identity

Dense, utilitarian, calm. Tonal layers separate tools from data; rotated-square "diamond" motifs mark the brand, bullets, and status points. All measurements render in a mono font; UI prose in a humanist sans.

## 2. Color

### Palette (tokens in `src/App.css`)

| Role | Token | Dark | Light | Usage |
|------|-------|------|-------|-------|
| Background | `--bg` | `#121714` | `#f7f5f1` | App content background |
| Surface 0 | `--surface-0` | `#161d1a` | `#eeebe5` | Status bar, deep wells |
| Surface 1 | `--surface-1` | `#1a221f` | `#ffffff` | Cards, tables, panels |
| Surface 2 | `--surface-2` | `#1c2622` | `#f2efe9` | Inputs, hover rows |
| Surface 3 | `--surface-3` | `#2a3530` | `#e4e0d8` | Scrollbar thumbs |
| Header | `--header-bg` | `#161d1a` | `#faf8f5` | View header bar |
| Status bar | `--statusbar-bg` | `#161d1a` | `#eeebe5` | Bottom status strip |
| Text primary | `--text` | `#eef3f1` | `#171c1a` | Body, headings |
| Text secondary | `--text-secondary` | `#c9d3cf` | `#3c4542` | Table data, secondary labels |
| Text muted | `--text-muted` | `#8f9b96` | `#6b7370` | Section labels, hints |
| Text faint | `--text-faint` | `#7f8b86` | `#8b938f` | Timestamps, placeholders |
| Border | `--border` | `rgba(255,255,255,.07)` | `rgba(23,28,26,.10)` | Card hairlines |
| Border strong | `--border-strong` | `rgba(255,255,255,.12)` | `rgba(23,28,26,.16)` | Inputs, chips |
| Border subtle | `--border-subtle` | `rgba(255,255,255,.05)` | `rgba(23,28,26,.05)` | Row dividers |
| Well | `--well` | `rgba(255,255,255,.07)` | `rgba(23,28,26,.06)` | Meter tracks, segmented bg |
| Accent | `--accent` | `#10b981` | `#0f7a5a` | Primary actions, good state |
| Accent bright | `--accent-bright` | `#10b981` | `#10b981` | Diamonds, live dots, meters |
| Accent soft | `--accent-soft` | `rgba(16,185,129,.14)` | `rgba(16,185,129,.14)` | Active/tinted backgrounds |
| Danger | `--danger` | `#d4695a` | `#c0503f` | Errors, loss |
| Warning | `--warning` | `#c98a2e` | `#c98a2e` | Warnings, secondary series |
| Text on accent | `--text-on-accent` | `#08231b` | `#ffffff` | Primary button text |

### Sidebar (both themes)

Dark green always: bg `#1b2521`, text `#cfd8d3`, active item `rgba(16,185,129,.14)` bg + `#eafaf4` text, brand `#f2f5f3`. Tokens: `--sidebar-*`.

### Status badge pairs

| Role | Background | Foreground (dark) | Foreground (light) |
|------|-----------|-------------------|--------------------|
| Success | `rgba(16,185,129,.14)` | `#34d399` | `#0f7a5a` |
| Warning | `rgba(201,138,46,.18/.16)` | `#d9a04a` | `#8a5f14` |
| Error | `rgba(192,80,63,.16/.10)` | `#e08473` | `#a8412f` |
| Neutral | `rgba(255,255,255,.08)` / `rgba(23,28,26,.06)` | `#c9d3cf` | `#3c4542` |

### Rules
- All colors reference a token; no raw hex in component styles (rgba derivations of the accent/amber/brick hues are allowed for heat scales).
- Surface hierarchy is tonal only; no drop shadows on panels.
- Accent is reserved for interactive elements, live state, and data highlights.

## 3. Typography

### Fonts
- UI: `"Archivo", system-ui, sans-serif` (`--font-ui`)
- Data: `"JetBrains Mono", ui-monospace, monospace` (`--font-mono`)
- Both are bundled via `@fontsource/*` (imported in `src/main.tsx`); no network fetch.

### Scale (from the mockups)

| Level | Spec | Usage |
|-------|------|-------|
| View title | 600 18px/1.2, ls −0.01em | `.vk-view-title` |
| Brand | 600 14.5px, ls −0.01em | Sidebar app name |
| Nav item | 500 12.5px | Sidebar links, tabs |
| Section label | 600 10px, ls .12em, uppercase | `.vk-section-title`, card headers |
| Table header | 600 9.5px, ls .1em, uppercase | `.vk-table-head`, `th` |
| Stat value | 600 19px mono (27px for tiles) | `.vk-stat-value` |
| Chip value | 500 13px mono | `.vk-chip-value` |
| Data rows | 400–500 11–11.5px mono | Tables, logs |
| Status bar | 400 10.5px mono | `.vk-statusbar` |

## 4. Spacing & Layout

- Sidebar: 216px fixed (icon rail below 720px).
- View header: `padding: 14px 20px`, hairline bottom border.
- View body: `padding: 18px 20px`, 16px gaps (`.vk-view-body`).
- Cards: 10px radius, 1px `--border`, padding 14–15px.
- Status bar: 27px min-height, mono, hairline top border.

## 5. Components

Shared primitives live in `src/components/ui/ui.tsx` with global `vk-*` classes in `src/components/ui/ui.css`:

- `Card`, `SectionHeader`, `ViewHeader`, `Chip`, `Button` (primary / secondary / outline-accent / outline-danger), `StatusBar`, `Stat`, `Meter`, `Diamond`, `Live`, `Segmented`.
- Class helpers: `.vk-table-head`, `.vk-table-row`, `.vk-badge*`, `.vk-view-body`, `.vk-statusbar*`.

### Sidebar
- Grouped nav (MEASURE / DISCOVER / MIKROTIK), 9px uppercase group labels, diamond bullets, `Ctrl N` kbd hints (shortcuts are wired in `App.tsx`), theme switcher at the bottom.

### Heat scales
Latency cells use green alpha by RTT (`rgba(accent, 0.16 + min(ms,20)/20*0.84)`) with brick (`--danger`) for loss and amber (`--warning`) for slow.

## 6. Motion & Interaction

- Hover/press transitions 100–150ms ease-out on background/color only.
- `vk-blink` animation for live dots; respects `prefers-reduced-motion` globally.

## 7. Depth & Surface

Tonal-shift only. No drop shadows on panels; separation comes from the warm/green neutral ramp plus 1px hairlines.

## 8. Accessibility

- WCAG 2.2 AA contrast targets on text pairs.
- Visible focus via browser default ring; full keyboard reachability including Ctrl+digit nav.
- Views keep semantic roles (tablist/tab/tabpanel, table markup, aria labels); do not trade them for divs.

### Accepted debt
- Native form controls rely on `color-scheme`; revisit if scrollbars render wrong on a platform.
