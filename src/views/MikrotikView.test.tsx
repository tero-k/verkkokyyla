// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { liveSnapshot, profiles, sessions } from "../hooks/useMikrotik.testFixtures"
import type { BackupResultDto, DeleteMikrotikProfileResultDto, MikrotikInterfaceDto, MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikStatusEvent, MikrotikTestConnectionDto, MikrotikVersionFirmwareResultDto } from "../lib/types"
import MikrotikView from "./MikrotikView"

type SnapshotHandler = (event: MikrotikSnapshotEvent) => void
type StatusHandler = (event: MikrotikStatusEvent) => void
type PlotData = readonly (readonly (number | null)[])[]

let snapshotHandler: SnapshotHandler | null = null

const plotMock = vi.hoisted(() => {
  const instances: { readonly target: HTMLElement; data: PlotData; setData: (data: PlotData) => void; destroy: () => void }[] = []
  class MockUPlot {
    readonly target: HTMLElement
    data: PlotData
    constructor(_options: object, data: PlotData, target: HTMLElement) {
      this.target = target; this.data = data; instances.push(this)
    }
    setData(data: PlotData): void { this.data = data }
    destroy(): void {}
  }
  return { instances, MockUPlot }
})

const ipc = vi.hoisted(() => ({
  mikrotikBackup: vi.fn<() => Promise<BackupResultDto>>(),
  mikrotikCheckUpdates: vi.fn<() => Promise<MikrotikVersionFirmwareResultDto>>(),
  mikrotikCreateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
  mikrotikDeleteProfile: vi.fn<() => Promise<DeleteMikrotikProfileResultDto>>(),
  mikrotikDeleteSession: vi.fn<(id: number) => Promise<void>>(),
  mikrotikFetchChangelog: vi.fn<() => Promise<{ readonly version: string; readonly changelog: string }>>(),
  mikrotikListProfiles: vi.fn<() => Promise<readonly MikrotikProfile[]>>(),
  mikrotikListSessions: vi.fn<() => Promise<readonly MikrotikSessionSummaryDto[]>>(),
  mikrotikLoadSession: vi.fn<(id: number) => Promise<MikrotikLoadedSessionDto>>(),
  mikrotikSetProfilePassword: vi.fn<() => Promise<void>>(),
  mikrotikStart: vi.fn<(profileId: number, onEvent: SnapshotHandler, onStatus: StatusHandler) => Promise<{ readonly sessionId: number; readonly profileId: number }>>(),
  mikrotikStop: vi.fn<() => Promise<{ readonly sessionId: number; readonly snapshotCount: number; readonly endedAt: string; readonly status: string }>>(),
  mikrotikTestConnection: vi.fn<() => Promise<MikrotikTestConnectionDto>>(),
  mikrotikUpdateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
}))

vi.mock("uplot/dist/uPlot.esm.js", () => ({ default: plotMock.MockUPlot }))
vi.mock("../theme", () => ({ useTheme: () => ({ mode: "dark", resolved: "dark", setMode: vi.fn() }) }))
vi.mock("../lib/ipc", () => ipc)
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn<() => Promise<string | null>>() }))

class ImmediateResizeObserver implements ResizeObserver {
  readonly #callback: ResizeObserverCallback
  constructor(callback: ResizeObserverCallback) { this.#callback = callback }
  observe(target: Element): void { this.#callback([{ target, contentRect: new DOMRectReadOnly(0, 0, 640, 160), borderBoxSize: [], contentBoxSize: [], devicePixelContentBoxSize: [] }], this) }
  unobserve(): void {}
  disconnect(): void {}
}

function secondInterface(rxBitsPerSecond: number | null): MikrotikInterfaceDto {
  return { ...liveSnapshot("2026-09-06T12:00:00Z", null).interfaces[0], name: "sfp1", rxByte: 3_000, txByte: 4_000, rxBitsPerSecond, txBitsPerSecond: 9_000 }
}

function snapshot(at: string, rxBitsPerSecond: number | null): MikrotikSnapshotEvent {
  const base = liveSnapshot(at, rxBitsPerSecond)
  return { ...base, interfaces: [base.interfaces[0], secondInterface(rxBitsPerSecond === null ? null : 8_000)] }
}

function loadedSession(): MikrotikLoadedSessionDto {
  const first = snapshot("2026-09-06T12:00:00Z", null).interfaces
  const second = snapshot("2026-09-06T12:00:10Z", null).interfaces.map((item) => ({ ...item, rxByte: item.rxByte === null ? null : item.rxByte + 500, txByte: item.txByte === null ? null : item.txByte + 250 }))
  return {
    session: { ...sessions[0], status: "completed", endedAt: "2026-09-06T12:01:00Z", boardName: "CCR2004" },
    snapshots: [
      {
        id: 1,
        sessionId: 22,
        at: "2026-09-06T12:00:00Z",
        cpuLoad: 20,
        memUsedBytes: 100,
        memTotalBytes: 200,
        uptime: "1h",
        boardName: "CCR2004-row",
        routerosVersion: "7.15.3-row",
        architectureName: "arm64-row",
        warning: null,
        sensorsJson: "[]",
        interfacesJson: JSON.stringify(first),
        vlansJson: null,
        bridgeVlansJson: null,
      },
      {
        id: 2,
        sessionId: 22,
        at: "2026-09-06T12:00:10Z",
        cpuLoad: 25,
        memUsedBytes: 120,
        memTotalBytes: 200,
        uptime: "1h10s",
        boardName: "CCR2004-row",
        routerosVersion: "7.15.3-row",
        architectureName: "arm64-row",
        warning: null,
        sensorsJson: "[]",
        interfacesJson: JSON.stringify(second),
        vlansJson: null,
        bridgeVlansJson: null,
      },
    ],
  }
}

function interfacePlot(): { readonly data: PlotData } | undefined {
  return plotMock.instances.find((plot) => plot.target.dataset.testid === "mikrotik-interface-graph")
}

beforeEach(() => {
  snapshotHandler = null; plotMock.instances.splice(0); vi.clearAllMocks(); vi.stubGlobal("ResizeObserver", ImmediateResizeObserver)
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(performance.now()), 0)
  ipc.mikrotikListProfiles.mockResolvedValue(profiles); ipc.mikrotikListSessions.mockResolvedValue(sessions); ipc.mikrotikLoadSession.mockResolvedValue(loadedSession())
  ipc.mikrotikStart.mockImplementation(async (profileId, onEvent) => { snapshotHandler = onEvent; return { sessionId: 31, profileId } })
  ipc.mikrotikStop.mockResolvedValue({ sessionId: 31, snapshotCount: 1, endedAt: "2026-09-06T12:01:00Z", status: "cancelled" })
  ipc.mikrotikDeleteSession.mockResolvedValue(); ipc.mikrotikTestConnection.mockResolvedValue({ boardName: "RB5009", routerosVersion: "7.16", architectureName: "arm64" })
})

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

describe("MikrotikView", () => {
  it("lands on Profiles with profile tools and persistent monitoring controls", async () => {
    render(<MikrotikView />)

    await waitFor(() => expect(screen.getByRole("option", { name: "lab-router" })).toBeTruthy())

    expect(screen.getByTestId("mikrotik-view")).toBeTruthy()
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Profiles",
      "Backups",
      "System",
      "Interfaces",
      "VLANs",
    ])
    expect(screen.getByRole("tab", { name: "Profiles" }).getAttribute("aria-selected")).toBe("true")
    const profilesPanel = screen.getByRole("tabpanel", { name: "Profiles" })
    expect(within(profilesPanel).getByLabelText("MikroTik profiles")).toBeTruthy()
    expect(within(profilesPanel).queryByTestId("mikrotik-backup-panel")).toBeNull()
    expect(within(profilesPanel).queryByText("Backups use the selected profile.")).toBeNull()
    expect(within(profilesPanel).queryByLabelText("MikroTik versions")).toBeNull()
    expect(screen.getByRole("button", { name: "Start" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy()
    const systemTab = screen.getByRole("tab", { name: "System" })
    expect(systemTab.id).toBe("mikrotik-tab-system")
    expect(systemTab.getAttribute("aria-controls")).toBe("mikrotik-panel-system")
    expect(screen.queryByRole("tab", { name: "Statistics" })).toBeNull()
    expect(screen.queryByRole("tabpanel", { name: "System" })).toBeNull()
  })

  it.each([
    { name: "Backups", testIds: ["mikrotik-backup-panel"], labels: ["MikroTik backups"] },
    { name: "System", testIds: ["mikrotik-status-cpu", "mikrotik-cpu-graph", "mikrotik-memory-graph", "mikrotik-session-panel"], labels: ["MikroTik versions"] },
    { name: "Interfaces", testIds: ["mikrotik-interface-table", "mikrotik-selected-interface", "mikrotik-interface-graph"], labels: [] },
    { name: "VLANs", testIds: ["mikrotik-vlan-panel"], labels: [] },
  ] as const)("shows the $name panel when its tab is selected", async ({ name, testIds, labels }) => {
    render(<MikrotikView />)
    await waitFor(() => expect(screen.getByRole("option", { name: "lab-router" })).toBeTruthy())

    fireEvent.click(screen.getByRole("tab", { name }))

    const panel = screen.getByRole("tabpanel", { name })
    const panelQueries = within(panel)
    expect(screen.getByRole("tab", { name }).getAttribute("aria-selected")).toBe("true")
    for (const testId of testIds) expect(panelQueries.getByTestId(testId)).toBeTruthy()
    for (const label of labels) expect(panelQueries.getByLabelText(label)).toBeTruthy()
    if (name === "System") {
      expect(panel.id).toBe("mikrotik-panel-system")
      expect(panel.getAttribute("aria-labelledby")).toBe("mikrotik-tab-system")
      expect(panelQueries.queryByTestId("mikrotik-selected-interface")).toBeNull()
      expect(panelQueries.queryByTestId("mikrotik-interface-graph")).toBeNull()
    }
    if (name === "Interfaces") {
      expect(panelQueries.getByText("Select an interface row to update the rate graph below.")).toBeTruthy()
    }
  })

  it("updates the live interface graph inside the Interfaces tab", async () => {
    render(<MikrotikView />)
    await waitFor(() => expect(screen.getByRole("option", { name: "lab-router" })).toBeTruthy())

    fireEvent.click(screen.getByRole("button", { name: "Start" }))
    await waitFor(() => expect(snapshotHandler).not.toBeNull())
    act(() => { snapshotHandler?.(snapshot("2026-09-06T12:00:00Z", null)); snapshotHandler?.(snapshot("2026-09-06T12:00:05Z", 1_500)) })
    fireEvent.click(screen.getByRole("tab", { name: "Interfaces" }))
    await waitFor(() => expect(screen.getByTestId("interface-row-sfp1")).toBeTruthy())
    fireEvent.click(screen.getByTestId("interface-row-sfp1"))

    await waitFor(() => expect(screen.getByTestId("mikrotik-selected-interface").textContent).toBe("Selected interface: sfp1"))
    expect(interfacePlot()?.data[1]).toEqual([null, 8_000])
    expect(interfacePlot()?.data[2]).toEqual([9_000, 9_000])
  })

  it("loads history into the same selected-interface graph buffers", async () => {
    render(<MikrotikView />)
    fireEvent.click(screen.getByRole("tab", { name: "System" }))
    await waitFor(() => expect(screen.getByTestId("mikrotik-open-session")).toBeTruthy())

    fireEvent.click(screen.getByTestId("mikrotik-open-session"))
    await waitFor(() => expect(screen.getByText("CCR2004-row")).toBeTruthy())
    fireEvent.click(screen.getByRole("tab", { name: "Interfaces" }))
    fireEvent.click(screen.getByTestId("interface-row-sfp1"))

    await waitFor(() => expect(screen.getByTestId("mikrotik-selected-interface").textContent).toBe("Selected interface: sfp1"))
    expect(ipc.mikrotikLoadSession).toHaveBeenCalledWith(22)
    expect(interfacePlot()?.data[1]).toEqual([null, 400])
    expect(interfacePlot()?.data[2]).toEqual([null, 200])
  })
})
