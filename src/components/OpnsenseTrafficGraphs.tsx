import { useEffect, useRef, useState, type CSSProperties } from "react"
import UPlot from "uplot/dist/uPlot.esm.js"
import {
  trafficSourceKey,
  type OpnsenseTrafficHistory,
  type OpnsenseTrafficSource,
} from "../lib/opnsenseTraffic"
import { useTheme } from "../theme"

import "uplot/dist/uPlot.min.css"
import styles from "./OpnsenseTrafficGraphs.module.css"

const GRAPH_HEIGHT = 170
const X_TICK_SPACE = 96
const COLOR_TOKENS = [
  "--accent",
  "--warning",
  "--graph-jitter",
  "--graph-loss",
  "--success-secondary",
  "--text-secondary",
] as const

type TrafficGraphsProps = { readonly history: OpnsenseTrafficHistory }
type ChartGroup = "interface" | "vpn"
type ChartRefs = Record<ChartGroup, UPlot | null>

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function graphAxisConfig(label: string): object {
  return {
    stroke: cssVar("--graph-axis"),
    grid: { stroke: cssVar("--graph-grid") },
    ticks: { stroke: cssVar("--graph-grid") },
    font: `10px "JetBrains Mono", ui-monospace, monospace`,
    label,
    labelColor: cssVar("--graph-axis"),
    labelSize: 13,
    size: 52,
    gap: 6,
  }
}

function xAxisConfig(): object {
  return {
    ...graphAxisConfig(""),
    space: X_TICK_SPACE,
    size: 36,
    values: (plot: UPlot, splits: readonly number[]) => {
      const xs = plot.data[0]
      const span = xs.length >= 2 ? xs[xs.length - 1] - xs[0] : 0
      return splits.map((value) => {
        const date = new Date(value * 1000)
        if (span > 180) {
          return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
        }
        return date.toLocaleTimeString(undefined, { hour12: false })
      })
    },
  }
}

function formatRateTick(value: number): string {
  const magnitude = Math.abs(value)
  if (magnitude >= 1_000_000_000) return `${Number((value / 1_000_000_000).toFixed(1))}G`
  if (magnitude >= 1_000_000) return `${Number((value / 1_000_000).toFixed(1))}M`
  if (magnitude >= 1000) return `${Number((value / 1000).toFixed(1))}K`
  return String(Math.round(value))
}

function formatRateLegend(value: number | null): string {
  return value === null ? "—" : `${formatRateTick(value)}bit/s`
}

function sourceLabel(source: OpnsenseTrafficSource): string {
  if (source.kind === "interface") return source.name
  if (source.kind === "wireguard") return `WireGuard ${source.interfaceName} / ${source.name}`
  return `OpenVPN ${source.name}`
}

function hasLineSegment(history: OpnsenseTrafficHistory, sources: readonly OpnsenseTrafficSource[]): boolean {
  return sources.some((source) => {
    const series = history.rates[trafficSourceKey(source)]
    return [series.rx, series.tx].some((values) =>
      values.some((value, index) => index > 0 && value !== null && values[index - 1] !== null),
    )
  })
}

export function OpnsenseTrafficGraphs({ history }: TrafficGraphsProps) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const interfaceRef = useRef<HTMLDivElement>(null)
  const vpnRef = useRef<HTMLDivElement>(null)
  const chartsRef = useRef<ChartRefs>({ interface: null, vpn: null })
  const [width, setWidth] = useState(0)
  const { resolved } = useTheme()
  const interfaceSources = history.selected.filter((source) => source.kind === "interface")
  const vpnSources = history.selected.filter((source) => source.kind !== "interface")
  const interfaceSelection = JSON.stringify(interfaceSources.map(trafficSourceKey))
  const vpnSelection = JSON.stringify(vpnSources.map(trafficSourceKey))

  useEffect(() => {
    const element = wrapperRef.current
    if (element === null) return
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) setWidth(Math.floor(entry.contentRect.width))
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (width <= 0) return

    const build = (
      sources: readonly OpnsenseTrafficSource[],
      target: HTMLDivElement | null,
    ): UPlot | null => {
      if (target === null) return null
      const series: uPlot.Series[] = [{}]
      for (const [index, source] of sources.entries()) {
        const color = cssVar(COLOR_TOKENS[index % COLOR_TOKENS.length])
        const label = sourceLabel(source)
        series.push(
          { label: `${label} RX`, stroke: color, width: 2, spanGaps: false, points: { show: false } },
          { label: `${label} TX`, stroke: color, width: 2, dash: [4, 3], spanGaps: false, points: { show: false } },
        )
      }
      return new UPlot(
        {
          width,
          height: GRAPH_HEIGHT,
          legend: { show: false },
          scales: { x: { time: true }, y: {} },
          axes: [xAxisConfig(), { ...graphAxisConfig("bit/s"), values: (_plot: UPlot, splits: readonly number[]) => splits.map(formatRateTick) }],
          series,
        },
        [[...history.times], ...series.slice(1).map(() => [])],
        target,
      )
    }

    chartsRef.current = {
      interface: build(interfaceSources, interfaceRef.current),
      vpn: build(vpnSources, vpnRef.current),
    }
    return () => {
      chartsRef.current.interface?.destroy()
      chartsRef.current.vpn?.destroy()
      chartsRef.current = { interface: null, vpn: null }
    }
    // Selection signatures deliberately exclude tick updates from chart reconstruction.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [width, resolved, interfaceSelection, vpnSelection])

  useEffect(() => {
    for (const [chart, sources] of [
      [chartsRef.current.interface, interfaceSources],
      [chartsRef.current.vpn, vpnSources],
    ] as const) {
      chart?.setData([
        [...history.times],
        ...sources.flatMap((source) => {
          const series = history.rates[trafficSourceKey(source)]
          return [[...series.rx], [...series.tx]]
        }),
      ])
    }
    // Selection signatures keep source identity aligned with the constructed charts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [history, width, interfaceSelection, vpnSelection])

  const panel = (
    group: ChartGroup,
    title: string,
    sources: readonly OpnsenseTrafficSource[],
    ref: React.Ref<HTMLDivElement>,
  ) => {
    const emptyMessage = group === "interface"
      ? "Add an interface to graph"
      : "Add a VPN peer or session to graph"
    return (
      <section className={styles.panel} aria-labelledby={`opnsense-${group}-traffic-title`}>
        <h2 className={styles.title} id={`opnsense-${group}-traffic-title`}>{title}</h2>
        {sources.length === 0 ? <p className={styles.message}>{emptyMessage}</p> : (
          <div className={styles.legend} aria-label={`${title} legend`}>
            {sources.map((source, index) => {
              const series = history.rates[trafficSourceKey(source)]
              return (
                <span
                  className={styles.legendItem}
                  key={trafficSourceKey(source)}
                  style={{ "--source-color": `var(${COLOR_TOKENS[index % COLOR_TOKENS.length]})` } as CSSProperties}
                >
                  <span className={styles.sourceName}>{sourceLabel(source)}</span>
                  <span>RX: {formatRateLegend(series.rx.at(-1) ?? null)}</span>
                  <span className={styles.txKey}>TX: {formatRateLegend(series.tx.at(-1) ?? null)}</span>
                </span>
              )
            })}
          </div>
        )}
        {sources.length > 0 && !hasLineSegment(history, sources)
          ? <p className={styles.message}>Waiting for another fresh sample</p>
          : null}
        <div
          className={styles.chart}
          data-testid={`opnsense-${group}-graph`}
          ref={ref}
          role="img"
          aria-label={`${title} receive and transmit rates over time`}
        />
      </section>
    )
  }

  return (
    <div className={styles.wrapper} ref={wrapperRef}>
      {panel("interface", "Interface traffic", interfaceSources, interfaceRef)}
      {panel("vpn", "VPN traffic", vpnSources, vpnRef)}
    </div>
  )
}
