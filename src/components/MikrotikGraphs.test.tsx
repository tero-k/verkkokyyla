// @vitest-environment jsdom
import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { MikrotikSnapshotEvent } from "../lib/types"
import type { MikrotikRateSeries } from "../lib/mikrotikSeries"
import { MikrotikGraphs } from "./MikrotikGraphs"

type PlotData = readonly (readonly (number | null)[])[]

type AxisOptions = {
  readonly values?: (
    plot: unknown,
    splits: readonly number[],
  ) => readonly string[]
}

type PlotOptions = {
  readonly axes?: readonly AxisOptions[]
}

const plotMock = vi.hoisted(() => {
  const instances: MockUPlot[] = []

  class MockUPlot {
    readonly options: PlotOptions
    readonly target: HTMLElement
    data: PlotData
    destroyed = false

    constructor(options: PlotOptions, data: PlotData, target: HTMLElement) {
      this.options = options
      this.target = target
      this.data = data
      instances.push(this)
    }

    setData(data: PlotData): void {
      this.data = data
    }

    destroy(): void {
      this.destroyed = true
    }
  }

  return { instances, MockUPlot }
})

vi.mock("uplot/dist/uPlot.esm.js", () => ({ default: plotMock.MockUPlot }))
vi.mock("../theme", () => ({
  useTheme: () => ({ mode: "dark", resolved: "dark", setMode: vi.fn() }),
}))

class ImmediateResizeObserver implements ResizeObserver {
  readonly #callback: ResizeObserverCallback

  constructor(callback: ResizeObserverCallback) {
    this.#callback = callback
  }

  observe(target: Element): void {
    const entry: ResizeObserverEntry = {
      target,
      contentRect: new DOMRectReadOnly(0, 0, 640, 160),
      borderBoxSize: [],
      contentBoxSize: [],
      devicePixelContentBoxSize: [],
    }
    this.#callback([entry], this)
  }

  unobserve(): void {}

  disconnect(): void {}
}

const firstAt = "2026-09-06T12:00:00Z"
const secondAt = "2026-09-06T12:00:05Z"

function snapshot(
  at: string,
  cpuLoad: number,
  memUsedBytes: number,
): MikrotikSnapshotEvent {
  return {
    event: "snapshot",
    sessionId: 22,
    at,
    resources: { cpuLoad, memUsedBytes, memTotalBytes: 400, uptime: "1h" },
    sensors: [],
    sensorsSupported: true,
    interfaces: [],
    vlans: null,
    bridgeVlans: null,
    warning: null,
  }
}

const snapshots: readonly MikrotikSnapshotEvent[] = [
  snapshot(firstAt, 20, 100),
  snapshot(secondAt, 35, 200),
]

const rateSeries: MikrotikRateSeries = {
  ether2: [
    { at: firstAt, rxBitsPerSecond: null, txBitsPerSecond: null },
    { at: secondAt, rxBitsPerSecond: 8_000, txBitsPerSecond: 4_000 },
  ],
}

beforeEach(() => {
  plotMock.instances.splice(0)
  vi.stubGlobal("ResizeObserver", ImmediateResizeObserver)
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe("MikrotikGraphs", () => {
  it("mounts three uPlot containers with CPU, memory, and selected-interface data", async () => {
    const view = render(
      <MikrotikGraphs
        snapshots={snapshots}
        rateSeries={rateSeries}
        selectedInterface="ether2"
      />,
    )

    await waitFor(() => expect(plotMock.instances).toHaveLength(3))

    const timestampData = [Date.parse(firstAt) / 1_000, Date.parse(secondAt) / 1_000]
    const cpuPlot = plotMock.instances.find(
      (plot) => plot.target.dataset.testid === "mikrotik-cpu-graph",
    )
    const memoryPlot = plotMock.instances.find(
      (plot) => plot.target.dataset.testid === "mikrotik-memory-graph",
    )
    const interfacePlot = plotMock.instances.find(
      (plot) => plot.target.dataset.testid === "mikrotik-interface-graph",
    )

    expect(view.getByTestId("mikrotik-cpu-graph")).toBeTruthy()
    expect(view.getByTestId("mikrotik-memory-graph")).toBeTruthy()
    expect(view.getByTestId("mikrotik-interface-graph")).toBeTruthy()
    expect(view.getByText("Interface traffic: ether2")).toBeTruthy()
    expect(cpuPlot?.data).toEqual([timestampData, [20, 35]])
    expect(memoryPlot?.data).toEqual([timestampData, [25, 50]])
    expect(interfacePlot?.data).toEqual([timestampData, [null, 8_000], [null, 4_000]])
  })

  it("compacts interface-rate ticks so the mobile y-axis remains legible", async () => {
    render(
      <MikrotikGraphs
        snapshots={snapshots}
        rateSeries={rateSeries}
        selectedInterface="ether2"
      />,
    )
    await waitFor(() => expect(plotMock.instances).toHaveLength(3))

    const interfacePlot = plotMock.instances.find(
      (plot) => plot.target.dataset.testid === "mikrotik-interface-graph",
    )
    const formatTicks = interfacePlot?.options.axes?.[1]?.values

    expect(formatTicks?.({}, [0, 950, 1_000, 1_500_000, 12_500_000])).toEqual([
      "0",
      "950",
      "1K",
      "1.5M",
      "12.5M",
    ])
  })

  it("destroys every uPlot instance on unmount", async () => {
    const view = render(
      <MikrotikGraphs
        snapshots={snapshots}
        rateSeries={rateSeries}
        selectedInterface="ether2"
      />,
    )
    await waitFor(() => expect(plotMock.instances).toHaveLength(3))

    view.unmount()

    expect(plotMock.instances.every((plot) => plot.destroyed)).toBe(true)
  })
})
