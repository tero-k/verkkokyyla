import { useEffect, useRef, useState } from "react"
import UPlot from "uplot/dist/uPlot.esm.js"
import { buildGraphData } from "../lib/graphData"
import type { ProbeRow } from "../lib/types"

import "uplot/dist/uPlot.min.css"
import styles from "./PingGraphs.module.css"

type PingGraphsProps = {
  readonly probes: readonly ProbeRow[]
}

export function PingGraphs({ probes }: PingGraphsProps) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const rttRef = useRef<HTMLDivElement>(null)
  const lossRef = useRef<HTMLDivElement>(null)
  const jitterRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const chartsRef = useRef<{
    rtt: UPlot | null
    loss: UPlot | null
    jitter: UPlot | null
  }>({ rtt: null, loss: null, jitter: null })

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
        title: "RTT",
        scales: { x: { time: true }, y: {} },
        axes: [{}, { label: "ms" }],
        series: [
          {},
          {
            label: "RTT",
            stroke: "#38bdf8",
            spanGaps: false,
            points: { show: false },
          },
          {
            label: "Loss",
            stroke: "#ef4444",
            points: { show: true, size: 3, fill: "#ef4444" },
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
        title: "Loss %",
        scales: { x: { time: true }, y: {} },
        axes: [{}, { label: "%" }],
        series: [
          {},
          {
            label: "Loss %",
            stroke: "#f87171",
            fill: "rgba(248, 113, 113, 0.2)",
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
        title: "Jitter",
        scales: { x: { time: true }, y: {} },
        axes: [{}, { label: "ms" }],
        series: [
          {},
          {
            label: "Jitter",
            stroke: "#a78bfa",
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
  }, [width])

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
    <div className={styles.wrapper} ref={wrapperRef}>
      <div className={styles.chart} ref={rttRef} />
      <div className={styles.chart} ref={lossRef} />
      <div className={styles.chart} ref={jitterRef} />
    </div>
  )
}
