import type { CSSProperties, ReactNode } from "react"
import "./ui.css"

/** Card panel: white/surface, 1px hairline border, 10px radius. */
export function Card({
  children,
  className = "",
  style,
  pad = false,
}: {
  children: ReactNode
  className?: string
  style?: CSSProperties
  pad?: boolean
}) {
  return (
    <div
      className={`vk-card${pad ? " vk-card-pad" : ""}${className ? ` ${className}` : ""}`}
      style={style}
    >
      {children}
    </div>
  )
}

/** Tiny uppercase letter-spaced section label with an optional right slot. */
export function SectionHeader({
  title,
  aside,
}: {
  title: ReactNode
  aside?: ReactNode
}) {
  return (
    <div className="vk-section">
      <span className="vk-section-title">{title}</span>
      {aside != null && <span className="vk-section-aside">{aside}</span>}
    </div>
  )
}

/** Top bar of a screen: title, optional mono subtitle, right-aligned actions. */
export function ViewHeader({
  title,
  subtitle,
  children,
}: {
  title?: ReactNode
  subtitle?: ReactNode
  children?: ReactNode
}) {
  return (
    <div className="vk-view-header">
      {title != null && <div className="vk-view-title">{title}</div>}
      {subtitle != null && <div className="vk-view-subtitle">{subtitle}</div>}
      {children}
    </div>
  )
}

/** Bordered label+value chip used in header toolbars (TARGET, options…). */
export function Chip({
  label,
  value,
  aside,
  style,
}: {
  label: ReactNode
  value: ReactNode
  aside?: ReactNode
  style?: CSSProperties
}) {
  return (
    <div className="vk-chip" style={style}>
      <span className="vk-chip-label">{label}</span>
      <span className="vk-chip-value">{value}</span>
      {aside != null && <span className="vk-chip-aside">{aside}</span>}
    </div>
  )
}

type ButtonVariant = "secondary" | "primary" | "outline-accent" | "outline-danger"

/** Mockup button. variant: primary (green), secondary (bordered),
 *  outline-accent (green outline), outline-danger (red outline). */
export function Button({
  children,
  variant = "secondary",
  small = false,
  className = "",
  ...rest
}: {
  children: ReactNode
  variant?: ButtonVariant
  small?: boolean
  className?: string
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const cls = [
    "vk-btn",
    variant !== "secondary" ? `vk-btn-${variant}` : "",
    small ? "vk-btn-sm" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ")
  return (
    <button type="button" className={cls} {...rest}>
      {children}
    </button>
  )
}

/** Bottom status strip of a screen. */
export function StatusBar({ children }: { children: ReactNode }) {
  return <div className="vk-statusbar">{children}</div>
}

/** Label over a mono value, mockup stat block. */
export function Stat({
  label,
  value,
  unit,
  sub,
  color,
  large = false,
}: {
  label: ReactNode
  value: ReactNode
  unit?: ReactNode
  sub?: ReactNode
  color?: string
  large?: boolean
}) {
  return (
    <div>
      <div className="vk-stat-label">{label}</div>
      <div
        className={`vk-stat-value${large ? " vk-stat-value-lg" : ""}`}
        style={color ? { color } : undefined}
      >
        {value}
        {unit != null && <span className="vk-stat-unit">{unit}</span>}
      </div>
      {sub != null && <div className="vk-stat-sub">{sub}</div>}
    </div>
  )
}

/** Thin labeled progress meter. */
export function Meter({
  label,
  value,
  pct,
  color,
}: {
  label: ReactNode
  value: ReactNode
  /** 0–100 */
  pct: number
  color?: string
}) {
  const width = `${Math.max(0, Math.min(100, pct))}%`
  return (
    <div>
      <div className="vk-meter-row">
        <span>{label}</span>
        <span className="vk-meter-value">{value}</span>
      </div>
      <div className="vk-meter-track">
        <div
          className="vk-meter-fill"
          style={{ width, ...(color ? { background: color } : {}) }}
        />
      </div>
    </div>
  )
}

/** Rotated-square brand/bullet motif. */
export function Diamond({
  color,
  small = false,
  style,
}: {
  color?: string
  small?: boolean
  style?: CSSProperties
}) {
  return (
    <span
      className={`vk-diamond${small ? " vk-diamond-sm" : ""}`}
      style={{ ...(color ? { background: color } : {}), ...style }}
    />
  )
}

/** Blinking live indicator. */
export function Live({ children }: { children?: ReactNode }) {
  return (
    <span className="vk-live">
      <span className="vk-live-dot" />
      {children ?? "live"}
    </span>
  )
}

/** Pill segmented control in a well. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
}: {
  options: readonly { value: T; label: ReactNode }[]
  value: T
  onChange: (value: T) => void
  ariaLabel?: string
}) {
  return (
    <div className="vk-segmented" role="group" aria-label={ariaLabel}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={o.value === value}
          className={`vk-segment${o.value === value ? " vk-segment-active" : ""}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}
