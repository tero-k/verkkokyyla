// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { filterLogEntries, LOG_BUFFER_CAP, useMikrotikLogs } from "./useMikrotikLogs"
import type { MikrotikLogEntry, MikrotikLogEvent, MikrotikLogStatusEvent } from "../lib/types"

type EventHandler = (event: MikrotikLogEvent) => void
type StatusHandler = (event: MikrotikLogStatusEvent) => void

let eventHandler: EventHandler | null = null
let statusHandler: StatusHandler | null = null

const ipc = vi.hoisted(() => ({
  mikrotikLogStart: vi.fn<
    (profileId: number, pollSeconds: number, onEvent: EventHandler, onStatus: StatusHandler) => Promise<{ readonly profileId: number }>
  >(),
  mikrotikLogStop: vi.fn<(profileId: number) => Promise<void>>(),
}))

vi.mock("../lib/ipc", () => ipc)

function entry(id: string, severity: MikrotikLogEntry["severity"], topics: readonly string[], message: string): MikrotikLogEntry {
  return { id, time: "12:52:24", topics, message, severity }
}

beforeEach(() => {
  eventHandler = null
  statusHandler = null
  vi.clearAllMocks()
  ipc.mikrotikLogStart.mockImplementation(async (profileId, _pollSeconds, onEvent, onStatus) => {
    eventHandler = onEvent
    statusHandler = onStatus
    return { profileId }
  })
  ipc.mikrotikLogStop.mockResolvedValue(undefined)
})

describe("filterLogEntries", () => {
  const entries: MikrotikLogEntry[] = [
    entry("*1", "info", ["system", "info"], "router rebooted"),
    entry("*2", "warning", ["dhcp", "warning"], "lease expired"),
    entry("*3", "error", ["system", "error"], "kernel failure"),
    entry("*4", "critical", ["system", "critical"], "disk full"),
    entry("*5", "debug", ["system", "debug"], "raw packet"),
  ]

  it("keeps everything for the 'all' filter", () => {
    expect(filterLogEntries(entries, "all", "", "")).toHaveLength(5)
  })

  it("'warnings' keeps warning, error and critical", () => {
    expect(filterLogEntries(entries, "warnings", "", "").map((e) => e.id)).toEqual(["*2", "*3", "*4"])
  })

  it("'errors' keeps error and critical only", () => {
    expect(filterLogEntries(entries, "errors", "", "").map((e) => e.id)).toEqual(["*3", "*4"])
  })

  it("filters by topic", () => {
    expect(filterLogEntries(entries, "all", "", "dhcp").map((e) => e.id)).toEqual(["*2"])
  })

  it("filters by case-insensitive text over message and topics", () => {
    expect(filterLogEntries(entries, "all", "KERNEL", "").map((e) => e.id)).toEqual(["*3"])
    expect(filterLogEntries(entries, "all", "dhcp", "").map((e) => e.id)).toEqual(["*2"])
    expect(filterLogEntries(entries, "all", "  ", "")).toHaveLength(5)
  })

  it("combines severity, topic and text filters", () => {
    expect(filterLogEntries(entries, "errors", "disk", "system").map((e) => e.id)).toEqual(["*4"])
    expect(filterLogEntries(entries, "errors", "disk", "dhcp")).toHaveLength(0)
  })
})

describe("useMikrotikLogs", () => {
  it("starts, collects entries, and stops on the stopped status", async () => {
    const { result } = renderHook(() => useMikrotikLogs())

    await act(async () => result.current.start(7))
    expect(ipc.mikrotikLogStart).toHaveBeenCalledWith(7, 2, expect.any(Function), expect.any(Function))
    expect(result.current.running).toBe(true)

    act(() => statusHandler?.({ event: "started", profileId: 7 }))
    act(() =>
      eventHandler?.({
        event: "entries",
        entries: [entry("*1", "info", ["system"], "boot"), entry("*2", "error", ["system", "error"], "boom")],
      }),
    )

    await waitFor(() => expect(result.current.entries).toHaveLength(2))
    expect(result.current.topics).toEqual(["error", "system"])

    await act(async () => result.current.stop())
    act(() => statusHandler?.({ event: "stopped" }))
    expect(result.current.running).toBe(false)
  })

  it("prepends newer batches so the newest entry stays on top", async () => {
    const { result } = renderHook(() => useMikrotikLogs())
    await act(async () => result.current.start(7))

    act(() => eventHandler?.({ event: "entries", entries: [entry("*1", "info", ["system"], "old")] }))
    await waitFor(() => expect(result.current.entries).toHaveLength(1))

    act(() =>
      eventHandler?.({
        event: "entries",
        entries: [entry("*2", "info", ["system"], "older in batch"), entry("*3", "info", ["system"], "newest")],
      }),
    )
    await waitFor(() => expect(result.current.entries).toHaveLength(3))
    expect(result.current.entries.map((e) => e.id)).toEqual(["*3", "*2", "*1"])
  })

  it("caps the buffer at LOG_BUFFER_CAP keeping the newest entries", async () => {
    const { result } = renderHook(() => useMikrotikLogs())
    await act(async () => result.current.start(7))

    const batch = Array.from({ length: LOG_BUFFER_CAP + 25 }, (_, index) =>
      entry(`*${index + 1}`, "info", ["system"], `entry ${index + 1}`),
    )
    act(() => eventHandler?.({ event: "entries", entries: batch }))

    await waitFor(() => expect(result.current.totalCount).toBe(LOG_BUFFER_CAP))
    // Newest on top: the highest id leads, the 25 oldest fell off the end.
    expect(result.current.entries[0].id).toBe(`*${LOG_BUFFER_CAP + 25}`)
    expect(result.current.entries[result.current.entries.length - 1].id).toBe("*26")
  })

  it("surfaces terminal errors and allows a restart", async () => {
    const { result } = renderHook(() => useMikrotikLogs())
    await act(async () => result.current.start(7))

    act(() => statusHandler?.({ event: "error", message: "log stream failed 3 polls in a row: boom" }))
    await waitFor(() => expect(result.current.running).toBe(false))
    expect(result.current.error).toContain("3 polls in a row")

    await act(async () => result.current.start(7))
    expect(result.current.running).toBe(true)
  })

  it("restart applies a new poll cadence to a live stream", async () => {
    const { result } = renderHook(() => useMikrotikLogs())
    await act(async () => result.current.start(7))
    expect(ipc.mikrotikLogStart).toHaveBeenLastCalledWith(7, 2, expect.any(Function), expect.any(Function))

    await act(async () => result.current.setPollSeconds(10))
    expect(result.current.pollSeconds).toBe(10)

    await act(async () => result.current.restart(7))
    expect(ipc.mikrotikLogStop).toHaveBeenCalledWith(7)
    expect(ipc.mikrotikLogStart).toHaveBeenLastCalledWith(7, 10, expect.any(Function), expect.any(Function))
    expect(result.current.running).toBe(true)
  })

  it("stop swallows an already-terminated stream error", async () => {
    ipc.mikrotikLogStop.mockRejectedValue(new Error("no log stream is running"))
    const { result } = renderHook(() => useMikrotikLogs())
    await act(async () => result.current.start(7))
    await act(async () => result.current.stop())
    expect(result.current.running).toBe(false)
  })
})
