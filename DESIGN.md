# Verkkokyylä Design System

## 1. Atmosphere & Identity

Verkkokyylä is a compact desktop network utility: dense, utilitarian, and calm. The signature is a quiet command surface — muted tonal layers separate tools from data, while small colored accents (latency values, status badges, graph strokes) carry meaning without noise.

## 2. Color

### Palette

| Role | Token | Dark | Light | Usage |
|------|-------|------|-------|-------|
| Background | `--bg` | `#18181b` | `#fafafa` | App root background |
| Surface 0 | `--surface-0` | `#111113` | `#f4f4f5` | Sidebar, deep panels |
| Surface 1 | `--surface-1` | `#1c1c1f` | `#ffffff` | Cards, tables, panels |
| Surface 2 | `--surface-2` | `#27272a` | `#e4e4e7` | Inputs, hover rows |
| Surface 3 | `--surface-3` | `#3f3f46` | `#d4d4d8` | Buttons, borders |
| Text primary | `--text` | `#e4e4e7` | `#18181b` | Body, headings |
| Text secondary | `--text-secondary` | `#d4d4d8` | `#3f3f46` | Secondary labels |
| Text muted | `--text-muted` | `#a1a1aa` | `#71717a` | Hints, timestamps |
| Text faint | `--text-faint` | `#71717a` | `#a1a1aa` | Placeholder, disabled |
| Border | `--border` | `#3f3f46` | `#d4d4d8` | Dividers, input borders |
| Accent | `--accent` | `#38bdf8` | `#0284c7` | Links, primary metric highlights |
| Accent blue | `--accent-blue` | `#60a5fa` | `#2563eb` | Secondary links |
| Danger | `--danger` | `#f87171` | `#dc2626` | Errors, destructive text |
| Danger secondary | `--danger-secondary` | `#ef4444` | `#dc2626` | Graph series, bad state |
| Warning | `--warning` | `#fbbf24` | `#d97706` | Warnings |
| Warning secondary | `--warning-secondary` | `#fde047` | `#b45309` | Warning accents |
| Success | `--success` | `#4ade80` | `#16a34a` | Good state |
| Success secondary | `--success-secondary` | `#86efac` | `#166534` | Success accents |
| Text on accent | `--text-on-accent` | `#eff6ff` | `#ffffff` | Primary button text |
| Overlay | `--overlay` | `rgba(0,0,0,0.7)` | `rgba(0,0,0,0.5)` | Modal backdrop |
| Warning soft | `--warning-soft` | `rgba(251,191,36,0.1)` | `rgba(217,119,6,0.1)` | Slow-row background |
| Warning soft 2 | `--warning-soft-2` | `rgba(251,191,36,0.2)` | `rgba(217,119,6,0.2)` | Slow badge background |
| Status success soft bg | `--status-success-soft-bg` | `rgba(34,197,94,0.08)` | `rgba(34,197,94,0.08)` | Traceroute same row |
| Status warning soft bg | `--status-warning-soft-bg` | `rgba(250,204,21,0.08)` | `rgba(250,204,21,0.08)` | Traceroute changed row |
| Status error soft bg | `--status-error-soft-bg` | `rgba(248,113,113,0.08)` | `rgba(248,113,113,0.08)` | Traceroute missing row |
| Graph RTT | `--graph-rtt` | `#38bdf8` | `#0284c7` | RTT series |
| Graph loss | `--graph-loss` | `#ef4444` | `#dc2626` | Loss series |
| Graph jitter | `--graph-jitter` | `#a78bfa` | `#7c3aed` | Jitter series |
| Graph loss fill | `--graph-loss-fill` | `rgba(248,113,113,0.2)` | `rgba(220,38,38,0.2)` | Loss area fill |
| Graph axis | `--graph-axis` | `var(--text-muted)` | `var(--text-muted)` | uPlot axes/labels |
| Graph grid | `--graph-grid` | `var(--border)` | `var(--border)` | uPlot grid |

### Status badge pairs

| Role | Background token | Foreground token | Dark bg | Dark fg | Light bg | Light fg |
|------|------------------|------------------|---------|---------|----------|----------|
| Success | `--status-success-bg` | `--status-success-text` | `#14532d` | `#86efac` | `#dcfce7` | `#166534` |
| Warning | `--status-warning-bg` | `--status-warning-text` | `#713f12` | `#fde047` | `#fef3c7` | `#92400e` |
| Error | `--status-error-bg` | `--status-error-text` | `#7f1d1d` | `#fca5a5` | `#fee2e2` | `#991b1b` |
| Info | `--status-info-bg` | `--status-info-text` | `#1e3a8a` | `#93c5fd` | `#dbeafe` | `#1e40af` |
| Neutral | `--status-neutral-bg` | `--status-neutral-text` | `#3f3f46` | `#d4d4d8` | `#f4f4f5` | `#3f3f46` |
| Error subtle | `--status-error-subtle-bg` | `--status-error-subtle-text` | `#450a0a` | `#fee2e2` | `#fef2f2` | `#7f1d1d` |
| Error panel | `--status-error-panel-bg` | `--status-error-panel-text` | `#2a0f0f` | `#fca5a5` | `#fef2f2` | `#991b1b` |

### Rules
- All colors reference a token from this table; no raw hex remains in component styles.
- Surface hierarchy uses tonal shifts; no heavy shadows.
- Accent colors are reserved for interactive elements and data highlights.

## 3. Typography

### Scale

| Level | Size | Weight | Usage |
|-------|------|--------|-------|
| Brand | 1.2rem | 700 | Sidebar app name |
| H1 | 1.4rem | 600 | View titles |
| Body | 1rem | 400 | Default text |
| Body sm | 0.875rem | 400 | Timestamps, hints |
| Caption | 0.75rem | 500 | Labels, overlines |

### Font Stack
- Primary: `system-ui, -apple-system, "Segoe UI", sans-serif`
- Mono: `ui-monospace, SFMono-Regular, "SF Mono", Consolas, "Liberation Mono", Menlo, monospace`

### Rules
- Body text never below 14px.
- `color: var(--text)` is explicit on interactive elements that might otherwise inherit browser defaults.

## 4. Spacing & Layout

### Base Unit
4px base unit; existing spacing stays unchanged.

### Grid
- Sidebar width: 10rem fixed.
- Content fills remaining viewport.
- Page padding: 1rem.

## 5. Components

### Sidebar
- Structure: vertical flex container with app brand, nav list, and theme switcher at bottom.
- Nav item: block link, full width, `border-radius: 0.25rem`.
- States:
  - default: transparent background, `var(--text-muted)` color
  - hover: `var(--surface-2)` background
  - active: `var(--surface-2)` background, `var(--text)` color, left accent border

### Theme Switcher
- Structure: three buttons in a row (Light / Dark / System).
- Selected state: `var(--surface-2)` background, `var(--text)` color.
- Unselected: transparent, `var(--text-muted)` color.

### Panel / Card
- Structure: `var(--surface-1)` background, `0.5rem` border radius, `1rem` padding.
- Variants: compact (session list), elevated (download result card).

### Button
- Primary: `var(--accent-blue)` background, white text.
- Secondary: `var(--surface-3)` background, `var(--text)` color.
- Danger: `var(--danger)` background, white text.

### Status Badge
- Small inline pill, uses the status token pairs above.

### Table
- Header: `var(--surface-1)` background, `var(--text-muted)` uppercase caption text.
- Rows: alternating `var(--surface-0)` / `var(--surface-1)`, hover `var(--surface-2)`.
- Text: `var(--text)` primary, `var(--text-muted)` secondary.

### Graph (uPlot)
- Axis stroke: `var(--text-muted)`.
- Grid stroke: `var(--border)`.
- Label color: `var(--text-muted)`.
- Series colors from accent/danger/success palette.

## 6. Motion & Interaction

### Timing
| Type | Duration | Easing | Usage |
|------|----------|--------|-------|
| Micro | 100ms | ease-out | Button press, toggle selection |
| Standard | 150ms | ease-in-out | Hover background change |

### Rules
- Only animate `background-color` and `color` on the theme switcher; prefer immediate theme switches elsewhere to avoid motion fatigue.
- Every interactive element has a visible hover state.
- Respect `prefers-reduced-motion`: skip transitions when enabled.

## 7. Depth & Surface

Strategy: tonal-shift only. Surfaces are separated by progressively lighter/darker shades of the same neutral ramp. No drop shadows on panels.

## 8. Accessibility Constraints & Accepted Debt

### Constraints
- WCAG 2.2 AA: body text 4.5:1, large text 3:1.
- Visible focus on every interactive element (browser default ring is acceptable).
- Full keyboard reachability for the theme switcher.

### Accepted Debt
| Item | Location | Why accepted | Owner / Exit |
|------|----------|--------------|--------------|
| Native form controls rely on `color-scheme` | Global | Tauri webview should render system controls; explicit tokens added where needed. | Revisit if scrollbars look wrong on a platform. |
