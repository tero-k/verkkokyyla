// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { liveSnapshot, profiles, sessions } from "../hooks/useMikrotik.testFixtures"
import type { BackupResultDto, DeleteMikrotikBackupResultDto, DeleteMikrotikProfileResultDto, MikrotikActiveSessionDto, MikrotikBackupRecordDto, MikrotikInterfaceDto, MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikStatusEvent, MikrotikTestConnectionDto, MikrotikVersionFirmwareResultDto } from "../lib/types"
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
  mikrotikDeleteBackup: vi.fn<(id: number) => Promise<DeleteMikrotikBackupResultDto>>(),
  mikrotikGetBackupDestination: vi.fn<() => Promise<string | null>>(),
  mikrotikSetBackupDestination: vi.fn<() => Promise<void>>(),
  mikrotikDiffBackups: vi.fn(),
  mikrotikCheckUpdates: vi.fn<() => Promise<MikrotikVersionFirmwareResultDto>>(),
  mikrotikCreateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
  mikrotikDeleteProfile: vi.fn<() => Promise<DeleteMikrotikProfileResultDto>>(),
  mikrotikDeleteSession: vi.fn<(id: number) => Promise<void>>(),
  mikrotikFetchChangelog: vi.fn<() => Promise<{ readonly version: string; readonly changelog: string }>>(),
  mikrotikListProfiles: vi.fn<() => Promise<readonly MikrotikProfile[]>>(),
  mikrotikListBackups: vi.fn<() => Promise<MikrotikBackupRecordDto[]>>(),
  mikrotikListSessions: vi.fn<() => Promise<readonly MikrotikSessionSummaryDto[]>>(),
  mikrotikLoadSession: vi.fn<(id: number) => Promise<MikrotikLoadedSessionDto>>(),
  mikrotikSetProfilePassword: vi.fn<() => Promise<void>>(),
  mikrotikStart: vi.fn<(profileId: number, onEvent: SnapshotHandler, onStatus: StatusHandler) => Promise<{ readonly sessionId: number; readonly profileId: number }>>(),
  mikrotikStop: vi.fn<(sessionId: number) => Promise<{ readonly sessionId: number; readonly snapshotCount: number; readonly endedAt: string; readonly status: string }>>(),
  mikrotikListActive: vi.fn<() => Promise<readonly MikrotikActiveSessionDto[]>>(),
  mikrotikTestConnection: vi.fn<() => Promise<MikrotikTestConnectionDto>>(),
  mikrotikUpdateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
}))

vi.mock("uplot/dist/uPlot.esm.js", () => ({ default: plotMock.MockUPlot }))
vi.mock("../theme", () => ({ useTheme: () => ({ mode: "dark", resolved: "dark", setMode: vi.fn() }) }))
vi.mock("../lib/ipc", () => ipc)
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn<() => Promise<string | null>>() }))

/** Terminal panel stub: real xterm can't run in jsdom, and the persistence
    contract under test is the VIEW's (which panel instances stay mounted
    across device switches), not the panel's internals. */
const terminalPanelStub = vi.hoisted(() => ({
  mountCounts: new Map<number, number>(),
  unmountCounts: new Map<number, number>(),
}))

vi.mock("../components/MikrotikTerminalPanel", async () => {
  const React = await import("react")
  return {
    terminalAccentFor: (profileId: number) => `hsl(${(profileId * 47) % 360} 75% 55%)`,
    MikrotikTerminalPanel: (props: {
      profileId: number
      profileName: string
      profileHost: string
      onActivated: (profileId: number) => void
      onDeactivated: (profileId: number) => void
    }) => {
      React.useEffect(() => {
        terminalPanelStub.mountCounts.set(props.profileId, (terminalPanelStub.mountCounts.get(props.profileId) ?? 0) + 1)
        return () => {
          terminalPanelStub.unmountCounts.set(props.profileId, (terminalPanelStub.unmountCounts.get(props.profileId) ?? 0) + 1)
        }
      }, [props.profileId])
      return React.createElement(
        "div",
        { "data-testid": `terminal-stub-${props.profileId}` },
        React.createElement("span", null, props.profileName),
        React.createElement("button", { onClick: () => props.onActivated(props.profileId) }, "stub-activate"),
        React.createElement("button", { onClick: () => props.onDeactivated(props.profileId) }, "stub-deactivate"),
      )
    },
  }
})

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
  terminalPanelStub.mountCounts.clear(); terminalPanelStub.unmountCounts.clear()
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(performance.now()), 0)
  ipc.mikrotikListProfiles.mockResolvedValue(profiles); ipc.mikrotikListSessions.mockResolvedValue(sessions); ipc.mikrotikLoadSession.mockResolvedValue(loadedSession())
  ipc.mikrotikListBackups.mockResolvedValue([]); ipc.mikrotikDeleteBackup.mockResolvedValue({ deleted: true, warnings: [] }); ipc.mikrotikGetBackupDestination.mockResolvedValue(null); ipc.mikrotikSetBackupDestination.mockResolvedValue(undefined)
  ipc.mikrotikStart.mockImplementation(async (profileId, onEvent) => { snapshotHandler = onEvent; return { sessionId: 31, profileId } })
  ipc.mikrotikStop.mockResolvedValue({ sessionId: 31, snapshotCount: 1, endedAt: "2026-09-06T12:01:00Z", status: "cancelled" })
  ipc.mikrotikListActive.mockResolvedValue([])
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
      "Logs",
      "Terminal",
    ])
    expect(screen.getByRole("tab", { name: "Profiles" }).getAttribute("aria-selected")).toBe("true")
    const profilesPanel = screen.getByRole("tabpanel", { name: "Profiles" })
    expect(within(profilesPanel).getByLabelText("MikroTik profiles")).toBeTruthy()
    expect(within(profilesPanel).queryByTestId("mikrotik-backup-panel")).toBeNull()
    expect(within(profilesPanel).queryByText("Backups use the selected profile.")).toBeNull()
    expect(within(profilesPanel).queryByLabelText("MikroTik versions")).toBeNull()
    expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull()
    expect(screen.queryByTestId("mikrotik-device-strip")).toBeNull()
    const systemTab = screen.getByRole("tab", { name: "System" })
    expect(systemTab.id).toBe("mikrotik-tab-system")
    expect(systemTab.getAttribute("aria-controls")).toBe("mikrotik-panel-system")
    expect(screen.queryByRole("tab", { name: "Statistics" })).toBeNull()
    expect(screen.queryByRole("tabpanel", { name: "System" })).toBeNull()
  })

  it.each([
    { name: "Backups", testIds: ["mikrotik-backup-panel", "mikrotik-backup-library"], labels: ["MikroTik backups", "MikroTik backup library"] },
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

    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
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

  it("shows one chip per connected device and switches dashboards between them", async () => {
    const secondProfile: MikrotikProfile = { ...profiles[0], id: 8, name: "core-switch" }
    const handlers = new Map<number, SnapshotHandler>()
    ipc.mikrotikListProfiles.mockResolvedValue([...profiles, secondProfile])
    ipc.mikrotikStart.mockImplementation(async (profileId, onEvent) => {
      handlers.set(profileId, onEvent)
      return { sessionId: profileId === 7 ? 31 : 32, profileId }
    })

    render(<MikrotikView />)
    await waitFor(() => expect(screen.getByRole("option", { name: "core-switch" })).toBeTruthy())

    // Connect the first (preselected) device.
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(screen.getByTestId("mikrotik-device-7")).toBeTruthy())
    act(() => { handlers.get(7)?.(liveSnapshot("2026-09-06T12:00:00Z", null)) })

    // Switch profile and connect the second device — the first keeps running.
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "8" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(screen.getByTestId("mikrotik-device-8")).toBeTruthy())
    const switchSnapshot = { ...liveSnapshot("2026-09-06T12:00:05Z", 7_000), sessionId: 32 }
    act(() => { handlers.get(8)?.(switchSnapshot) })

    // Two chips; the selected device's snapshot is on the dashboard.
    expect(screen.getByTestId("mikrotik-device-strip")).toBeTruthy()
    expect(screen.getAllByTestId(/^mikrotik-device-/).length).toBeGreaterThanOrEqual(2)
    await waitFor(() => expect(screen.getByTestId("mikrotik-status-cpu")).toBeTruthy())
    fireEvent.click(screen.getByRole("tab", { name: "System" }))
    await waitFor(() => expect(screen.getByText("RB5009-live")).toBeTruthy())

    // Click chip 7 → dashboard shows the first device's data again.
    fireEvent.click(screen.getByTestId("mikrotik-device-7").querySelector("button") as HTMLButtonElement)
    await waitFor(() => expect(screen.getAllByText("RB5009-live").length).toBeGreaterThanOrEqual(1))

    // Per-device disconnect stops only that device.
    const disconnectButtons = screen.getAllByRole("button", { name: "Disconnect" })
    expect(disconnectButtons.length).toBe(2)
    fireEvent.click(disconnectButtons[0])
    await waitFor(() => expect(ipc.mikrotikStop).toHaveBeenCalledWith(31))
    expect(screen.getByTestId("mikrotik-device-8")).toBeTruthy()
  })

  it("keeps an activated terminal mounted across device switches", async () => {
    const secondProfile: MikrotikProfile = { ...profiles[0], id: 8, name: "core-switch" }
    ipc.mikrotikListProfiles.mockResolvedValue([...profiles, secondProfile])
    render(<MikrotikView />)
    await waitFor(() => expect(screen.getByRole("option", { name: "core-switch" })).toBeTruthy())

    fireEvent.click(screen.getByRole("tab", { name: "Terminal" }))
    // The preselected device gets the ephemeral panel with its identity.
    await waitFor(() => expect(screen.getByTestId("terminal-stub-7")).toBeTruthy())
    expect(screen.getByTestId("terminal-stub-7").textContent).toContain("lab-router")

    // Activate device 7's terminal, then switch to device 8.
    fireEvent.click(within(screen.getByTestId("terminal-stub-7")).getByText("stub-activate"))
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "8" } })

    // Device 7's panel stays mounted (hidden); device 8 gets its own panel.
    await waitFor(() => expect(screen.getByTestId("terminal-stub-8")).toBeTruthy())
    expect(screen.getByTestId("terminal-stub-7").closest("[hidden]")).not.toBeNull()
    expect(terminalPanelStub.unmountCounts.get(7) ?? 0).toBe(0)

    // Switching back reuses the SAME instance — no remount, no reconnect.
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "7" } })
    await waitFor(() => expect(screen.getByTestId("terminal-stub-7").closest("[hidden]")).toBeNull())
    expect(terminalPanelStub.mountCounts.get(7)).toBe(1)

    // Deactivation drops the device from the persistent set; the still-
    // selected profile keeps the same (now ephemeral) instance mounted.
    fireEvent.click(within(screen.getByTestId("terminal-stub-7")).getByText("stub-deactivate"))
    expect(terminalPanelStub.unmountCounts.get(7) ?? 0).toBe(0)

    // Switching away now unmounts the ephemeral panel.
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "8" } })
    await waitFor(() => expect(terminalPanelStub.unmountCounts.get(7) ?? 0).toBe(1))
  })

  it("warns when the visible terminal switches devices, with a session silencer", async () => {
    const secondProfile: MikrotikProfile = { ...profiles[0], id: 8, name: "core-switch" }
    ipc.mikrotikListProfiles.mockResolvedValue([...profiles, secondProfile])
    render(<MikrotikView />)
    await waitFor(() => expect(screen.getByRole("option", { name: "core-switch" })).toBeTruthy())

    fireEvent.click(screen.getByRole("tab", { name: "Terminal" }))
    await waitFor(() => expect(screen.getByTestId("terminal-stub-7")).toBeTruthy())

    // Activate device 7's terminal; switching to device 8 (no terminal) does NOT warn.
    fireEvent.click(within(screen.getByTestId("terminal-stub-7")).getByText("stub-activate"))
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "8" } })
    await waitFor(() => expect(screen.getByTestId("terminal-stub-8")).toBeTruthy())
    expect(screen.queryByTestId("terminal-switch-warning")).toBeNull()

    // Activate device 8's terminal, then switch back to 7 — the warning
    // names the connection now in front of the user.
    fireEvent.click(within(screen.getByTestId("terminal-stub-8")).getByText("stub-activate"))
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "7" } })
    await waitFor(() => expect(screen.getByTestId("terminal-switch-warning")).toBeTruthy())
    expect(screen.getByTestId("terminal-switch-warning").textContent).toContain("lab-router")

    // Dismissed without silencing → the next switch warns again.
    fireEvent.click(screen.getByTestId("terminal-switch-warning-dismiss"))
    await waitFor(() => expect(screen.queryByTestId("terminal-switch-warning")).toBeNull())
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "8" } })
    await waitFor(() =>
      expect(screen.getByTestId("terminal-switch-warning").textContent).toContain("core-switch"),
    )

    // Silenced for the session → no more warnings until a restart.
    fireEvent.click(screen.getByLabelText("Don't warn again this session"))
    fireEvent.click(screen.getByTestId("terminal-switch-warning-dismiss"))
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "7" } })
    expect(screen.queryByTestId("terminal-switch-warning")).toBeNull()
  })
})
