import { useEffect, useRef, useState } from "react"
import UPlot from "uplot/dist/uPlot.esm.js"
import { buildGraphData } from "../lib/graphData"
import type { ProbeRow } from "../lib/types"
import { useTheme } from "../theme"
import { Card, SectionHeader } from "./ui/ui"

import "uplot/dist/uPlot.min.css"
import styles from "./PingGraphs.module.css"

type PingGraphsProps = {
  readonly probes: readonly ProbeRow[]
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function graphAxisConfig(label: string): object {
  return {
    stroke: cssVar("--graph-axis"),
    grid: { stroke: cssVar("--graph-grid") },
    ticks: { stroke: cssVar("--graph-grid") },
    font: `10px ${cssVar("--font-mono")}`,
    label,
    labelColor: cssVar("--graph-axis"),
    labelSize: 12,
  }
}

export function PingGraphs({ probes }: PingGraphsProps) {
  const rttRef = useRef<HTMLDivElement>(null)
  const lossRef = useRef<HTMLDivElement>(null)
  const jitterRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const { resolved } = useTheme()
  const chartsRef = useRef<{
    rtt: UPlot | null
    loss: UPlot | null
    jitter: UPlot | null
  }>({ rtt: null, loss: null, jitter: null })

  useEffect(() => {
    const element = rttRef.current
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
      rttRef.current === null ||
      lossRef.current === null ||
      jitterRef.current === null
    ) {
      return
    }

    const rttChart = new UPlot(
      {
        width,
        height: 160,
        legend: { show: false },
        scales: { x: { time: true }, y: {} },
        axes: [graphAxisConfig(""), graphAxisConfig("ms")],
        series: [
          {},
          {
            label: "RTT",
            stroke: cssVar("--graph-rtt"),
            width: 2,
            spanGaps: false,
            points: { show: false },
          },
          {
            label: "Loss",
            stroke: cssVar("--graph-loss"),
            points: { show: true, size: 3, fill: cssVar("--graph-loss") },
          },
        ],
      },
      [[], [], []],
      rttRef.current,
    )

    const lossChart = new UPlot(
      {
        width,
        height: 120,
        legend: { show: false },
        scales: { x: { time: true }, y: {} },
        axes: [graphAxisConfig(""), graphAxisConfig("%")],
        series: [
          {},
          {
            label: "Loss %",
            stroke: cssVar("--graph-loss"),
            width: 2,
            fill: cssVar("--graph-loss-fill"),
            points: { show: false },
          },
        ],
      },
      [[], []],
      lossRef.current,
    )

    const jitterChart = new UPlot(
      {
        width,
        height: 120,
        legend: { show: false },
        scales: { x: { time: true }, y: {} },
        axes: [graphAxisConfig(""), graphAxisConfig("ms")],
        series: [
          {},
          {
            label: "Jitter",
            stroke: cssVar("--graph-jitter"),
            width: 2,
            points: { show: false },
          },
        ],
      },
      [[], []],
      jitterRef.current,
    )

    chartsRef.current = { rtt: rttChart, loss: lossChart, jitter: jitterChart }

    return () => {
      rttChart.destroy()
      lossChart.destroy()
      jitterChart.destroy()
      chartsRef.current = { rtt: null, loss: null, jitter: null }
    }
  }, [width, resolved])

  useEffect(() => {
    const { rtt: rttChart, loss: lossChart, jitter: jitterChart } =
      chartsRef.current
    if (rttChart === null || lossChart === null || jitterChart === null) {
      return
    }

    const data = buildGraphData(probes, width)

    const rttXs = data.rtt.map((point) => point.x)
    const rttYs = data.rtt.map((point) => point.y)
    const markerYs = data.rtt.map((point) =>
      point.y === null ? (data.globalMax ?? 0) : null,
    )
    rttChart.setData([rttXs, rttYs, markerYs])

    const lossXs = data.loss.map((point) => point.x)
    const lossPct = data.loss.map((point) =>
      point.totalCount > 0 ? (point.lossCount / point.totalCount) * 100 : 0,
    )
    lossChart.setData([lossXs, lossPct])

    const jitterXs = data.jitter.map((point) => point.x)
    const jitterYs = data.jitter.map((point) => point.y)
    jitterChart.setData([jitterXs, jitterYs])
  }, [probes, width])

  return (
    <div className={styles.wrapper}>
      <Card className={styles.chartCard}>
        <div className={styles.chartHeader}>
          <SectionHeader title="Round-trip time" aside="milliseconds" />
        </div>
        <div
          className={styles.chart}
          ref={rttRef}
          role="img"
          aria-label="Round-trip time chart"
        />
      </Card>
      <Card className={styles.chartCard}>
        <div className={styles.chartHeader}>
          <SectionHeader title="Packet loss" aside="percent" />
        </div>
        <div
          className={styles.chart}
          ref={lossRef}
          role="img"
          aria-label="Packet loss chart"
        />
      </Card>
      <Card className={styles.chartCard}>
        <div className={styles.chartHeader}>
          <SectionHeader title="Jitter" aside="milliseconds" />
        </div>
        <div
          className={styles.chart}
          ref={jitterRef}
          role="img"
          aria-label="Jitter chart"
        />
      </Card>
    </div>
  )
}
