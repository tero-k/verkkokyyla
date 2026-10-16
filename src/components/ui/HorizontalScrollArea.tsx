import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react"
import styles from "./HorizontalScrollArea.module.css"

type HorizontalScrollAreaProps = {
  readonly label: string
  readonly children: ReactNode
  readonly className?: string
  readonly outerClassName?: string
  readonly unbounded?: boolean
  readonly scrollRef?: { current: HTMLDivElement | null }
}

/** Preserve native wheel/touch scrolling and show a draggable control even
 * when the OS hides overlay scrollbars. Only appears when content overflows. */
export function HorizontalScrollArea({ label, children, className, outerClassName, unbounded = false, scrollRef }: HorizontalScrollAreaProps) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const [metrics, setMetrics] = useState({ max: 0, position: 0, thumb: 28 })

  useLayoutEffect(() => {
    const node = containerRef.current
    if (node === null) return
    const update = () => {
      const max = Math.max(0, node.scrollWidth - node.clientWidth)
      const thumb = max > 0 ? Math.max(28, Math.round(node.clientWidth * node.clientWidth / node.scrollWidth)) : 28
      setMetrics((prev) =>
        prev.max === max && prev.position === node.scrollLeft && prev.thumb === thumb
          ? prev : { max, position: node.scrollLeft, thumb },
      )
    }
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update)
    let mutations: MutationObserver | null = null
    if (observer !== null) {
      observer.observe(node)
      if (node.firstElementChild !== null) observer.observe(node.firstElementChild)
    } else {
      // Older webviews still track text/row changes and window resizes.
      mutations = new MutationObserver(update)
      mutations.observe(node, { childList: true, subtree: true, characterData: true })
      window.addEventListener("resize", update)
    }
    node.addEventListener("scroll", update)
    update()
    return () => {
      node.removeEventListener("scroll", update)
      observer?.disconnect()
      mutations?.disconnect()
      if (observer === null) window.removeEventListener("resize", update)
    }
  }, [])

  return (
    <div className={`${styles.outer} ${unbounded ? "" : styles.bounded} ${outerClassName ?? ""}`}>
      <div className={`${styles.viewport} ${className ?? ""}`} ref={(node) => {
        containerRef.current = node
        if (scrollRef !== undefined) scrollRef.current = node
      }}>
        {children}
      </div>
      {metrics.max > 0 ? (
        <input
          type="range"
          className={styles.control}
          aria-label={`Scroll ${label} horizontally`}
          min={0}
          max={metrics.max}
          step={1}
          value={metrics.position}
          style={{ "--scroll-thumb-width": `${metrics.thumb}px` } as CSSProperties}
          onChange={(event) => {
            if (containerRef.current !== null) containerRef.current.scrollLeft = Number(event.currentTarget.value)
          }}
        />
      ) : null}
    </div>
  )
}
