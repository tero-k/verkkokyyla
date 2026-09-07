import { useEffect, useRef, useState } from "react"
import UPlot from "uplot/dist/uPlot.esm.js"
import type { MikrotikRateSeries } from "../lib/mikrotikSeries"
import type { MikrotikSnapshotEvent } from "../lib/types"
import { useTheme } from "../theme"

import "uplot/dist/uPlot.min.css"
import styles from "./MikrotikGraphs.module.css"

const GRAPH_HEIGHT = 160
const AXIS_LABEL_SIZE = 13
const X_TICK_SPACE = 96

export type MikrotikGraphsProps = {
  readonly snapshots: readonly MikrotikSnapshotEvent[]
  readonly rateSeries: MikrotikRateSeries
  readonly selectedInterface: string | null
}

type MikrotikCharts = {
  readonly cpu: UPlot | null
  readonly memory: UPlot | null
  readonly interfaceRates: UPlot | null
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function graphAxisConfig(label: string): object {
  return {
    stroke: cssVar("--graph-axis"),
    grid: { stroke: cssVar("--graph-grid") },
    ticks: { stroke: cssVar("--graph-grid") },
    font: `${AXIS_LABEL_SIZE}px system-ui, sans-serif`,
    label,
    labelColor: cssVar("--graph-axis"),
    labelSize: AXIS_LABEL_SIZE,
    size: 48,
    gap: 6,
  }
}

function xAxisConfig(): object {
  return {
    ...graphAxisConfig(""),
    space: X_TICK_SPACE,
    size: 36,
    values: (_plot: UPlot, splits: readonly number[]) =>
      splits.map((value) => `:${String(new Date(value * 1_000).getUTCSeconds()).padStart(2, "0")}`),
  }
}

function formatRateTick(value: number): string {
  const magnitude = Math.abs(value)
  if (magnitude >= 1_000_000_000) {
    return `${Number((value / 1_000_000_000).toFixed(1))}G`
  }
  if (magnitude >= 1_000_000) {
    return `${Number((value / 1_000_000).toFixed(1))}M`
  }
  if (magnitude >= 1_000) {
    return `${Number((value / 1_000).toFixed(1))}K`
  }
  return String(Math.round(value))
}

function rateAxisConfig(): object {
  return {
    ...graphAxisConfig("bit/s"),
    space: 48,
    incrs: [500_000, 1_000_000, 2_000_000],
    values: (_plot: UPlot, splits: readonly number[]) =>
      splits.map(formatRateTick),
  }
}

function formatPercentLegend(value: number | null): string {
  return value === null ? "-" : `${Number(value.toFixed(1))}%`
}

function formatRateLegend(value: number | null): string {
  return value === null ? "-" : `${formatRateTick(value)}bit/s`
}

function timestampSeconds(at: string, fallback: number): number {
  const timestamp = Date.parse(at)
  return Number.isFinite(timestamp) ? timestamp / 1_000 : fallback
}

function finiteValue(value: number | null): number | null {
  return value !== null && Number.isFinite(value) ? value : null
}

function memoryPercent(snapshot: MikrotikSnapshotEvent): number | null {
  const used = snapshot.resources?.memUsedBytes ?? null
  const total = snapshot.resources?.memTotalBytes ?? null
  if (
    used === null ||
    total === null ||
    !Number.isFinite(used) ||
    !Number.isFinite(total) ||
    total <= 0
  ) {
    return null
  }
  return (used / total) * 100
}

export function MikrotikGraphs({
  snapshots,
  rateSeries,
  selectedInterface,
}: MikrotikGraphsProps) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const cpuRef = useRef<HTMLDivElement>(null)
  const memoryRef = useRef<HTMLDivElement>(null)
  const interfaceRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const { resolved } = useTheme()
  const latestSnapshot = snapshots.at(-1) ?? null
  const points = selectedInterface === null ? [] : (rateSeries[selectedInterface] ?? [])
  const latestPoint = points.at(-1) ?? null
  const chartsRef = useRef<MikrotikCharts>({
    cpu: null,
    memory: null,
    interfaceRates: null,
  })

  useEffect(() => {
    const element = wrapperRef.current
    if (element === null) return

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setWidth(Math.floor(entry.contentRect.width))
      }
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (
      width === 0 ||
      cpuRef.current === null ||
      memoryRef.current === null ||
      interfaceRef.current === null
    ) {
      return
    }

    const cpuChart = new UPlot(
      {
        width,
        height: GRAPH_HEIGHT,
        legend: { show: false },
        scales: { x: { time: true }, y: { range: [0, 100] } },
        axes: [xAxisConfig(), graphAxisConfig("%")],
        series: [
          {},
          {
            label: "CPU",
            stroke: cssVar("--accent"),
            spanGaps: false,
            points: { show: false },
          },
        ],
      },
      [[], []],
      cpuRef.current,
    )

    const memoryChart = new UPlot(
      {
        width,
        height: GRAPH_HEIGHT,
        legend: { show: false },
        scales: { x: { time: true }, y: { range: [0, 100] } },
        axes: [xAxisConfig(), graphAxisConfig("%")],
        series: [
          {},
          {
            label: "Memory",
            stroke: cssVar("--graph-rtt"),
            spanGaps: false,
            points: { show: false },
          },
        ],
      },
      [[], []],
      memoryRef.current,
    )

    const interfaceChart = new UPlot(
      {
        width,
        height: GRAPH_HEIGHT,
        legend: { show: false },
        scales: { x: { time: true }, y: {} },
        axes: [xAxisConfig(), rateAxisConfig()],
        series: [
          {},
          {
            label: "RX",
            stroke: cssVar("--accent"),
            width: 3,
            spanGaps: false,
            points: { show: true, size: 8, stroke: cssVar("--accent"), fill: cssVar("--accent") },
          },
          {
            label: "TX",
            stroke: cssVar("--success-secondary"),
            width: 3,
            dash: [10, 6],
            spanGaps: false,
            points: { show: true, size: 3, stroke: cssVar("--success-secondary"), fill: cssVar("--surface-1") },
          },
        ],
      },
      [[], [], []],
      interfaceRef.current,
    )

    chartsRef.current = {
      cpu: cpuChart,
      memory: memoryChart,
      interfaceRates: interfaceChart,
    }

    return () => {
      cpuChart.destroy()
      memoryChart.destroy()
      interfaceChart.destroy()
      chartsRef.current = {
        cpu: null,
        memory: null,
        interfaceRates: null,
      }
    }
  }, [width, resolved])

  useEffect(() => {
    const { cpu, memory, interfaceRates } = chartsRef.current
    if (cpu === null || memory === null || interfaceRates === null) return

    const snapshotTimes = snapshots.map((snapshot, index) =>
      timestampSeconds(snapshot.at, index),
    )
    const cpuValues = snapshots.map((snapshot) =>
      finiteValue(snapshot.resources?.cpuLoad ?? null),
    )
    const memoryValues = snapshots.map(memoryPercent)
    cpu.setData([snapshotTimes, cpuValues])
    memory.setData([snapshotTimes, memoryValues])

    const rateTimes = points.map((point, index) =>
      timestampSeconds(point.at, index),
    )
    interfaceRates.setData([
      rateTimes,
      points.map((point) => finiteValue(point.rxBitsPerSecond)),
      points.map((point) => finiteValue(point.txBitsPerSecond)),
    ])
  }, [snapshots, rateSeries, selectedInterface, width])

  return (
    <div className={styles.wrapper} ref={wrapperRef}>
      <section className={styles.panel} aria-labelledby="mikrotik-cpu-title">
        <h2 className={styles.title} id="mikrotik-cpu-title">
          CPU load
        </h2>
        <p className={styles.legend}>CPU: {formatPercentLegend(latestSnapshot?.resources?.cpuLoad ?? null)}</p>
        <div
          className={styles.chart}
          data-testid="mikrotik-cpu-graph"
          ref={cpuRef}
          role="img"
          aria-label="CPU load percentage over time"
        />
      </section>
      <section className={styles.panel} aria-labelledby="mikrotik-memory-title">
        <h2 className={styles.title} id="mikrotik-memory-title">
          Memory usage
        </h2>
        <p className={styles.legend}>Memory: {formatPercentLegend(latestSnapshot === null ? null : memoryPercent(latestSnapshot))}</p>
        <div
          className={styles.chart}
          data-testid="mikrotik-memory-graph"
          ref={memoryRef}
          role="img"
          aria-label="Memory usage percentage over time"
        />
      </section>
      <section className={styles.panel} aria-labelledby="mikrotik-interface-title">
        <h2 className={styles.title} id="mikrotik-interface-title">
          Interface traffic: {selectedInterface ?? "No interface selected"}
        </h2>
        <div className={styles.legend} aria-label="Interface traffic legend">
          <span className={styles.rxKey}>RX: {formatRateLegend(latestPoint?.rxBitsPerSecond ?? null)}</span>
          <span className={styles.txKey}>TX: {formatRateLegend(latestPoint?.txBitsPerSecond ?? null)}</span>
        </div>
        <div
          className={styles.chart}
          data-interface={selectedInterface ?? ""}
          data-testid="mikrotik-interface-graph"
          ref={interfaceRef}
          role="img"
          aria-label="Selected interface receive and transmit rates over time"
        />
      </section>
    </div>
  )
}
