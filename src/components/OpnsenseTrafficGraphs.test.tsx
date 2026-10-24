// @vitest-environment jsdom
import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { trafficSourceKey, type OpnsenseTrafficHistory, type OpnsenseTrafficSource } from "../lib/opnsenseTraffic"
import { OpnsenseTrafficGraphs } from "./OpnsenseTrafficGraphs"

type PlotData = readonly (readonly (number | null)[])[]
type PlotSeries = { readonly label?: string; readonly dash?: readonly number[]; readonly stroke?: string }
type PlotOptions = { readonly series?: readonly PlotSeries[]; readonly width?: number }

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
    setData(data: PlotData): void { this.data = data }
    destroy(): void { this.destroyed = true }
  }
  return { instances, MockUPlot }
})

vi.mock("uplot/dist/uPlot.esm.js", () => ({ default: plotMock.MockUPlot }))
vi.mock("../theme", () => ({ useTheme: () => ({ mode: "dark", resolved: "dark", setMode: vi.fn() }) }))

let observedWidth = 640
const resizeObservers = new Set<ControlledResizeObserver>()
class ControlledResizeObserver implements ResizeObserver {
  readonly #callback: ResizeObserverCallback
  #target: Element | null = null
  constructor(callback: ResizeObserverCallback) { this.#callback = callback; resizeObservers.add(this) }
  observe(target: Element): void {
    this.#target = target
    this.emit(observedWidth)
  }
  emit(width: number): void {
    if (this.#target === null) return
    const entry: ResizeObserverEntry = {
      target: this.#target,
      contentRect: new DOMRectReadOnly(0, 0, width, 170),
      borderBoxSize: [], contentBoxSize: [], devicePixelContentBoxSize: [],
    }
    this.#callback([entry], this)
  }
  unobserve(): void {}
  disconnect(): void { this.#target = null; resizeObservers.delete(this) }
}

const igb0: OpnsenseTrafficSource = { kind: "interface", name: "igb0" }
const lagg0: OpnsenseTrafficSource = { kind: "interface", name: "lagg0" }
const wg: OpnsenseTrafficSource = { kind: "wireguard", interfaceName: "wg0", name: "remote office" }
const road: OpnsenseTrafficSource = { kind: "openvpn", name: "road-1" }
const times = [Date.parse("2026-10-24T12:00:00Z") / 1000, Date.parse("2026-10-24T12:00:05Z") / 1000]

function series(rx: readonly (number | null)[], tx: readonly (number | null)[]) {
  return { rx, tx }
}

function history(
  selected: readonly OpnsenseTrafficSource[] = [igb0, lagg0, wg, road],
): OpnsenseTrafficHistory {
  return {
    selected,
    times: selected.length === 0 ? [] : times,
    rates: {
      [trafficSourceKey(igb0)]: series([null, 1600], [null, 800]),
      [trafficSourceKey(lagg0)]: series([800, 2400], [400, 1200]),
      [trafficSourceKey(wg)]: series([null, 3200], [null, 1600]),
      [trafficSourceKey(road)]: series([1000, 4800], [500, 2400]),
    },
    baselines: {},
  }
}

function plot(testId: string) {
  return [...plotMock.instances].reverse().find((candidate) => candidate.target.dataset.testid === testId && !candidate.destroyed)
}

beforeEach(() => {
  plotMock.instances.splice(0)
  resizeObservers.clear()
  observedWidth = 640
  vi.stubGlobal("ResizeObserver", ControlledResizeObserver)
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe("OpnsenseTrafficGraphs", () => {
  it("places all selected RX/TX traces on their interface or VPN graph", async () => {
    const view = render(<OpnsenseTrafficGraphs history={history()} />)
    await waitFor(() => expect(plotMock.instances).toHaveLength(2))

    expect(plot("opnsense-interface-graph")?.data).toEqual([
      times, [null, 1600], [null, 800], [800, 2400], [400, 1200],
    ])
    expect(plot("opnsense-vpn-graph")?.data).toEqual([
      times, [null, 3200], [null, 1600], [1000, 4800], [500, 2400],
    ])
    expect(view.getByText("WireGuard wg0 / remote office")).toBeTruthy()
    expect(view.getByText("OpenVPN road-1")).toBeTruthy()
    expect(view.getByText("RX: 1.6Kbit/s")).toBeTruthy()
    expect(view.getAllByText("TX: 800bit/s")).toHaveLength(1)

    const options = plot("opnsense-interface-graph")?.options.series
    expect(options?.[1]).toMatchObject({ label: "igb0 RX" })
    expect(options?.[2]).toMatchObject({ label: "igb0 TX", dash: [4, 3] })
    expect(options?.[1]?.stroke).toBe(options?.[2]?.stroke)
  })

  it("reconfigures after removing one source while preserving the other", async () => {
    const view = render(<OpnsenseTrafficGraphs history={history([igb0, lagg0])} />)
    await waitFor(() => expect(plotMock.instances).toHaveLength(2))
    const previous = plot("opnsense-interface-graph")

    view.rerender(<OpnsenseTrafficGraphs history={history([lagg0])} />)
    await waitFor(() => expect(previous?.destroyed).toBe(true))
    expect(view.queryByText("igb0")).toBeNull()
    expect(view.getByText("lagg0")).toBeTruthy()
    expect(plot("opnsense-interface-graph")?.data).toEqual([times, [800, 2400], [400, 1200]])
  })

  it("recreates populated charts when a hidden panel regains width", async () => {
    render(<OpnsenseTrafficGraphs history={history([igb0])} />)
    await waitFor(() => expect(plotMock.instances).toHaveLength(2))
    for (const observer of resizeObservers) observer.emit(0)
    await waitFor(() => expect(plotMock.instances.every((candidate) => candidate.destroyed)).toBe(true))

    for (const observer of resizeObservers) observer.emit(420)
    await waitFor(() => expect(plot("opnsense-interface-graph")?.options.width).toBe(420))
    expect(plot("opnsense-interface-graph")?.data).toEqual([times, [null, 1600], [null, 800]])
  })

  it("shows selection and sampling prompts only when needed", async () => {
    const view = render(<OpnsenseTrafficGraphs history={history([])} />)
    expect(view.getByText("Add an interface to graph")).toBeTruthy()
    expect(view.getByText("Add a VPN peer or session to graph")).toBeTruthy()

    const waiting = history([igb0])
    view.rerender(<OpnsenseTrafficGraphs history={{ ...waiting, rates: { [trafficSourceKey(igb0)]: series([null, null], [null, null]) } }} />)
    expect(view.getByText("Waiting for another fresh sample")).toBeTruthy()
    view.rerender(<OpnsenseTrafficGraphs history={history([lagg0])} />)
    expect(view.queryByText("Waiting for another fresh sample")).toBeNull()
  })

  it("destroys every actual uPlot instance on unmount", async () => {
    const view = render(<OpnsenseTrafficGraphs history={history()} />)
    await waitFor(() => expect(plotMock.instances).toHaveLength(2))
    view.unmount()
    expect(plotMock.instances.every((candidate) => candidate.destroyed)).toBe(true)
  })
})
