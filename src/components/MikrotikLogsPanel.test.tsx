// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { MikrotikLogEvent, MikrotikLogStatusEvent } from "../lib/types"
import { MikrotikLogsPanel } from "./MikrotikLogsPanel"

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

afterEach(cleanup)

describe("MikrotikLogsPanel", () => {
  it("shows the idle empty state and disables Start without a profile", () => {
    render(<MikrotikLogsPanel profileId={null} />)
    expect(screen.getByTestId("mikrotik-logs-empty").textContent).toContain("Connect to the stream to see router logs.")
    const start = screen.getByRole("button", { name: "Connect" })
    expect((start as HTMLButtonElement).disabled).toBe(true)
  })

  it("streams entries and highlights error and warning rows", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))

    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalledWith(7, 2, expect.any(Function), expect.any(Function)))
    eventHandler?.({
      event: "entries",
      entries: [
        { id: "*1", time: "12:52:24", topics: ["system", "info"], message: "router rebooted", severity: "info" },
        { id: "*2", time: "12:53:00", topics: ["dhcp", "warning"], message: "lease expired", severity: "warning" },
        { id: "*3", time: "12:54:00", topics: ["system", "error"], message: "kernel failure", severity: "error" },
      ],
    })

    const rows = await screen.findAllByTestId("mikrotik-log-row")
    expect(rows).toHaveLength(3)
    // Newest on top: the batch arrives oldest-first and is reversed.
    expect(rows[0].dataset.severity).toBe("error")
    expect(rows[1].dataset.severity).toBe("warning")
    expect(rows[2].dataset.severity).toBe("info")
    expect(rows[0].textContent).toContain("kernel failure")
  })

  it("severity segmented filter narrows the visible rows", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalled())

    eventHandler?.({
      event: "entries",
      entries: [
        { id: "*1", time: "12:52:24", topics: ["system", "info"], message: "boot", severity: "info" },
        { id: "*2", time: "12:53:00", topics: ["system", "error"], message: "boom", severity: "error" },
      ],
    })
    await waitFor(() => expect(screen.getAllByTestId("mikrotik-log-row")).toHaveLength(2))

    fireEvent.click(screen.getByRole("button", { name: "Errors" }))
    const rows = screen.getAllByTestId("mikrotik-log-row")
    expect(rows).toHaveLength(1)
    expect(rows[0].textContent).toContain("boom")
  })

  it("stop cancels the stream", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalled())
    statusHandler?.({ event: "started", profileId: 7 })

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }))
    await waitFor(() => expect(ipc.mikrotikLogStop).toHaveBeenCalledWith(7))
  })

  it("free-text search filters rows and highlights the matched word", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalled())
    eventHandler?.({
      event: "entries",
      entries: [
        { id: "*1", time: "12:52:24", topics: ["system", "info"], message: "router rebooted", severity: "info" },
        { id: "*2", time: "12:53:00", topics: ["system", "error"], message: "kernel failure", severity: "error" },
      ],
    })
    await waitFor(() => expect(screen.getAllByTestId("mikrotik-log-row")).toHaveLength(2))

    fireEvent.change(screen.getByLabelText("Filter log messages"), { target: { value: "KERNEL" } })
    const rows = screen.getAllByTestId("mikrotik-log-row")
    expect(rows).toHaveLength(1)
    const marks = rows[0].querySelectorAll("mark")
    expect(marks).toHaveLength(1)
    expect(marks[0].textContent).toBe("kernel")
  })

  it("changing the refresh frequency while live restarts the stream", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalledWith(7, 2, expect.any(Function), expect.any(Function)))
    statusHandler?.({ event: "started", profileId: 7 })

    fireEvent.change(screen.getByLabelText("Refresh frequency"), { target: { value: "10" } })
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenLastCalledWith(7, 10, expect.any(Function), expect.any(Function)))
    expect(ipc.mikrotikLogStop).toHaveBeenCalled()
  })

  it("lists seen topics in the topic filter", async () => {
    render(<MikrotikLogsPanel profileId={7} />)
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikLogStart).toHaveBeenCalled())
    eventHandler?.({
      event: "entries",
      entries: [
        { id: "*1", time: "12:52:24", topics: ["caps", "info"], message: "join", severity: "info" },
      ],
    })
    const topicSelect = await screen.findByLabelText("Topic filter")
    expect(within(topicSelect as HTMLElement).getByRole("option", { name: "caps" })).toBeDefined()
  })
})
