// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { useMikrotik } from "./useMikrotik"
import { liveSnapshot, loadedMikrotikSession, profiles, sessions, updateResult } from "./useMikrotik.testFixtures"
import type { BackupResultDto, MikrotikActiveSessionDto, MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikStatusEvent, MikrotikVersionFirmwareResultDto } from "../lib/types"

type SnapshotHandler = (event: MikrotikSnapshotEvent) => void
type StatusHandler = (event: MikrotikStatusEvent) => void

let snapshotHandler: SnapshotHandler | null = null
let statusHandler: StatusHandler | null = null

const ipc = vi.hoisted(() => ({
  mikrotikListProfiles: vi.fn<() => Promise<readonly MikrotikProfile[]>>(),
  mikrotikStart: vi.fn<
    (
      profileId: number,
      onEvent: SnapshotHandler,
      onStatus: StatusHandler,
    ) => Promise<{ readonly sessionId: number; readonly profileId: number }>
  >(),
  mikrotikStop: vi.fn<(sessionId: number) => Promise<{ readonly sessionId: number; readonly snapshotCount: number; readonly endedAt: string; readonly status: string }>>(),
  mikrotikListActive: vi.fn<() => Promise<readonly MikrotikActiveSessionDto[]>>(),
  mikrotikListSessions: vi.fn<() => Promise<readonly MikrotikSessionSummaryDto[]>>(),
  mikrotikLoadSession: vi.fn<(id: number) => Promise<MikrotikLoadedSessionDto>>(),
  mikrotikDeleteSession: vi.fn<(id: number) => Promise<void>>(),
  mikrotikCheckUpdates: vi.fn<(profileId: number) => Promise<MikrotikVersionFirmwareResultDto>>(),
  mikrotikFetchChangelog: vi.fn<(version: string) => Promise<{ readonly version: string; readonly changelog: string }>>(),
  mikrotikBackup: vi.fn<
    (
      profileId: number,
      destinationDir: string,
      backupName: string,
      password: string | undefined,
      includeRsc: boolean,
      overwrite: boolean,
    ) => Promise<BackupResultDto>
  >(),
}))

vi.mock("../lib/ipc", () => ipc)

beforeEach(() => {
  snapshotHandler = null
  statusHandler = null
  vi.clearAllMocks()
  ipc.mikrotikListProfiles.mockResolvedValue(profiles)
  ipc.mikrotikListSessions.mockResolvedValue(sessions)
  ipc.mikrotikStart.mockImplementation(async (profileId, onEvent, onStatus) => {
    snapshotHandler = onEvent
    statusHandler = onStatus
    return { sessionId: 31, profileId }
  })
  ipc.mikrotikStop.mockResolvedValue({ sessionId: 31, snapshotCount: 1, endedAt: "2026-09-06T12:01:00Z", status: "cancelled" })
  ipc.mikrotikListActive.mockResolvedValue([])
  ipc.mikrotikDeleteSession.mockResolvedValue(); ipc.mikrotikCheckUpdates.mockResolvedValue(updateResult)
  ipc.mikrotikFetchChangelog.mockResolvedValue({ version: "7.17", changelog: "fixed" }); ipc.mikrotikBackup.mockResolvedValue({ backupPath: "C:/tmp/lab.backup", exportPath: null, cleanupWarnings: [] })
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(performance.now()), 0)
})

describe("useMikrotik", () => {
  it("starts a session and flushes buffered snapshot events into state", async () => {
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toEqual(profiles))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    snapshotHandler?.(liveSnapshot("2026-09-06T12:00:00Z", null)); snapshotHandler?.(liveSnapshot("2026-09-06T12:00:05Z", 1_500))

    expect(result.current.snapshotHistory).toHaveLength(0)

    await waitFor(() => expect(result.current.snapshotHistory).toHaveLength(2))
    expect(result.current.running).toBe(true)
    expect(result.current.latestSnapshot?.at).toBe("2026-09-06T12:00:05Z")
    expect(result.current.rateSeries.ether1).toEqual([{ at: "2026-09-06T12:00:00Z", rxBitsPerSecond: null, txBitsPerSecond: 2_000 }, { at: "2026-09-06T12:00:05Z", rxBitsPerSecond: 1_500, txBitsPerSecond: 2_000 }])
    expect(result.current.vlans).toHaveLength(1); expect(result.current.sensors).toHaveLength(1)
  })

  it("keeps sessions alive on unmount (devices survive navigation)", async () => {
    const { result, unmount } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    unmount()

    expect(ipc.mikrotikStop).not.toHaveBeenCalled()
  })

  it("reattaches an active session discovered after reload as detached", async () => {
    ipc.mikrotikListActive.mockResolvedValue([{ sessionId: 31, profileId: 7 }])
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.activeDevices).toHaveLength(1))
    expect(result.current.activeDevices[0]).toMatchObject({ profileId: 7, sessionId: 31, running: true, attached: false })
  })

  it("exposes action callbacks and keeps backup passwords transient", async () => {
    const refreshed = [{ ...profiles[0], id: 8, name: "refetched" }]
    ipc.mikrotikListProfiles.mockResolvedValueOnce(profiles).mockResolvedValueOnce(refreshed)
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toEqual(profiles))
    expect([result.current.refreshProfiles, result.current.checkUpdates, result.current.fetchChangelog, result.current.runBackup].every((action) => typeof action === "function")).toBe(true)
    await act(async () => result.current.refreshProfiles())
    await act(async () => result.current.checkUpdates())
    await act(async () => { await result.current.fetchChangelog("7.17") })
    await act(async () => { await result.current.runBackup({ profileId: 7, destinationDir: "C:/tmp", backupName: "lab", password: "transient-secret", includeRsc: true, overwrite: false }) })

    expect(result.current.profiles).toEqual(refreshed)
    expect(ipc.mikrotikCheckUpdates).toHaveBeenCalledWith(7)
    expect(ipc.mikrotikFetchChangelog).toHaveBeenCalledWith("7.17")
    expect(ipc.mikrotikBackup).toHaveBeenCalledWith(7, "C:/tmp", "lab", "transient-secret", true, false)
    expect(JSON.stringify(result.current)).not.toContain("transient-secret")
  })

  it("stops the active session", async () => {
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    await act(async () => result.current.stop())

    expect(ipc.mikrotikStop).toHaveBeenCalledWith(31); expect(result.current.running).toBe(false)
  })

  it("refreshes sessions after deleting a session", async () => {
    const refreshed = [{ ...sessions[0], id: 23 }]
    ipc.mikrotikListSessions.mockResolvedValueOnce(sessions).mockResolvedValueOnce(refreshed)
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.sessions).toEqual(sessions))
    await act(async () => result.current.deleteSession(22))

    await waitFor(() => expect(result.current.sessions).toEqual(refreshed))
  })

  it("surfaces status-channel errors and clears running state", async () => {
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    act(() => statusHandler?.({ event: "error", sessionId: 31, message: "core failed" }))

    expect(result.current.error).toBe("core failed"); expect(result.current.running).toBe(false)
  })

  it("consumes live version-firmware status events", async () => {
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    act(() => statusHandler?.({ event: "version-firmware", sessionId: 31, ...updateResult }))

    expect(result.current.updateStatus).toEqual(updateResult.updateStatus)
    expect(result.current.firmwareStatus).toEqual(updateResult.firmwareStatus)
  })

  it("loads historical session state including retained VLANs and session metadata", async () => {    const loaded: MikrotikLoadedSessionDto = loadedMikrotikSession()
    ipc.mikrotikLoadSession.mockResolvedValue(loaded)
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.sessions).toHaveLength(1))
    await act(async () => result.current.loadSession(22))

    expect(result.current.loadedSession?.session.boardName).toBe("CCR2004")
    expect(result.current.loadedSession?.session.routerosVersion).toBe("7.15.3")
    expect(result.current.loadedSession?.session.architectureName).toBe("arm64")
    expect(result.current.latestSnapshot?.resources).toMatchObject({ cpuLoad: 31, uptime: "1h7s" })
    expect(result.current.rateSeries.ether1?.[1]).toMatchObject({ rxBitsPerSecond: 800, txBitsPerSecond: 800 })
    expect(result.current.vlans).toEqual([{ name: "vlan20", vlanId: 20, interface: "bridge", running: true, disabled: false }])
    expect(result.current.bridgeVlans).toEqual(liveSnapshot("2026-09-06T12:00:00Z", null).bridgeVlans)
    expect(result.current.sensors).toEqual([{ name: "voltage", value: 24.2, unit: "V", kind: "voltage" }])
    expect(result.current.updateStatus).toEqual(updateResult.updateStatus)
    expect(result.current.firmwareStatus).toEqual(updateResult.firmwareStatus)
  })

  it("does not invoke start twice while one is in flight", async () => {
    let resolveStart:
      | ((dto: { readonly sessionId: number; readonly profileId: number }) => void)
      | null = null
    ipc.mikrotikStart.mockImplementation((_profileId, onEvent, onStatus) => {
      snapshotHandler = onEvent
      statusHandler = onStatus
      return new Promise((resolve) => {
        resolveStart = resolve
      })
    })
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => {
      void result.current.start()
    })
    await act(async () => {
      void result.current.start()
    })

    expect(ipc.mikrotikStart).toHaveBeenCalledTimes(1)
    await act(async () => {
      resolveStart?.({ sessionId: 31, profileId: profiles[0].id })
    })
  })

  it("ignores stale status events from a previous session after restart", async () => {
    let nextSessionId = 31
    ipc.mikrotikStart.mockImplementation(async (profileId, onEvent, onStatus) => {
      snapshotHandler = onEvent
      statusHandler = onStatus
      const sessionId = nextSessionId
      nextSessionId = 32
      return { sessionId, profileId }
    })
    const { result } = renderHook(() => useMikrotik())

    await waitFor(() => expect(result.current.profiles).toHaveLength(1))
    await act(async () => result.current.selectProfile(profiles[0]))
    await act(async () => result.current.start())
    // First session fails; a restart begins (the mock now yields session 32).
    act(() => statusHandler?.({ event: "error", sessionId: 31, message: "core failed" }))
    expect(result.current.running).toBe(false)
    await act(async () => result.current.start())

    expect(result.current.running).toBe(true)
    expect(result.current.error).toBe("")

    // A delayed terminal event from the OLD session must not kill the new one.
    act(() => statusHandler?.({ event: "stopped", sessionId: 31, snapshotCount: 3 }))

    expect(result.current.running).toBe(true)
    expect(result.current.error).toBe("")
  })
})
