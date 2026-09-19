// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { MikrotikTerminalPanel } from "./MikrotikTerminalPanel"

type DataHandler = (chunk: string) => void

let dataHandler: DataHandler | null = null

/** xterm and the fit addon are heavy DOM/canvas consumers — mock them and
    observe what the panel does with them (writes, disposals, input). */
const xtermMock = vi.hoisted(() => {
  const state = {
    writes: [] as string[],
    dataHandler: null as ((data: string) => void) | null,
    disposed: false,
  }
  class MockTerminal {
    cols = 80
    rows = 24
    open() {}
    loadAddon() {}
    write(data: Uint8Array) {
      state.writes.push(new TextDecoder().decode(data))
    }
    onData(handler: (data: string) => void) {
      state.dataHandler = handler
      return {
        dispose() {
          if (state.dataHandler === handler) state.dataHandler = null
        },
      }
    }
    dispose() {
      state.disposed = true
    }
  }
  class MockFitAddon {
    fit() {}
  }
  return { state, MockTerminal, MockFitAddon }
})

const ipc = vi.hoisted(() => ({
  mikrotikTerminalOpen: vi.fn<
    (
      profileId: number,
      cols: number,
      rows: number,
      onData: DataHandler,
    ) => Promise<{ readonly terminalId: number; readonly profileId: number }>
  >(),
  mikrotikTerminalWrite: vi.fn<(terminalId: number, data: string) => Promise<void>>(),
  mikrotikTerminalResize: vi.fn<(terminalId: number, cols: number, rows: number) => Promise<void>>(),
  mikrotikTerminalClose: vi.fn<(terminalId: number) => Promise<void>>(),
}))

vi.mock("../lib/ipc", () => ipc)
vi.mock("@xterm/xterm", () => ({ Terminal: xtermMock.MockTerminal }))
vi.mock("@xterm/addon-fit", () => ({ FitAddon: xtermMock.MockFitAddon }))

beforeEach(() => {
  dataHandler = null
  xtermMock.state.writes = []
  xtermMock.state.dataHandler = null
  xtermMock.state.disposed = false
  vi.clearAllMocks()
  ipc.mikrotikTerminalOpen.mockImplementation(
    async (_profileId, _cols, _rows, onData) => {
      dataHandler = onData
      return { terminalId: 7, profileId: 1 }
    },
  )
  ipc.mikrotikTerminalWrite.mockResolvedValue(undefined)
  ipc.mikrotikTerminalResize.mockResolvedValue(undefined)
  ipc.mikrotikTerminalClose.mockResolvedValue(undefined)
})

afterEach(cleanup)

function renderPanel() {
  const props = {
    profileId: 1,
    profileName: "Lab router",
    profileHost: "router.lab",
    onActivated: vi.fn(),
    onDeactivated: vi.fn(),
  }
  return { ...render(<MikrotikTerminalPanel {...props} />), props }
}

describe("MikrotikTerminalPanel", () => {
  it("shows the identity tag and an idle activation hint", () => {
    renderPanel()
    const identity = screen.getByTestId("mikrotik-terminal-identity")
    expect(identity.textContent).toContain("Lab router")
    expect(identity.textContent).toContain("router.lab")
    expect(screen.getByTestId("mikrotik-terminal-empty").textContent).toContain(
      "No terminal session. Connect opens a shell on Lab router (router.lab).",
    )
    expect((screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled).toBe(false)
  })

  it("opens a terminal, decodes output chunks, and encodes input", async () => {
    const { props } = renderPanel()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))

    await waitFor(() =>
      expect(ipc.mikrotikTerminalOpen).toHaveBeenCalledWith(1, 80, 24, expect.any(Function)),
    )

    // Backend output arrives as a base64 chunk and lands in xterm as bytes.
    dataHandler?.(btoa("[admin@MikroTik] > "))
    await waitFor(() =>
      expect(xtermMock.state.writes.join("")).toContain("[admin@MikroTik] > "),
    )

    // Keyboard input is encoded to base64 for the backend write.
    xtermMock.state.dataHandler?.("interface print\r")
    await waitFor(() =>
      expect(ipc.mikrotikTerminalWrite).toHaveBeenCalledWith(7, btoa("interface print\r")),
    )

    expect((screen.getByRole("button", { name: "Disconnect" }) as HTMLButtonElement).disabled).toBe(
      false,
    )
    expect(props.onActivated).toHaveBeenCalledWith(1)
  })

  it("closes the backend terminal on unmount", async () => {
    const { unmount } = renderPanel()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikTerminalOpen).toHaveBeenCalled())

    unmount()
    await waitFor(() => expect(ipc.mikrotikTerminalClose).toHaveBeenCalledWith(7))
  })

  it("reports deactivation on a deliberate disconnect", async () => {
    const { props } = renderPanel()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() =>
      expect(ipc.mikrotikTerminalOpen).toHaveBeenCalledWith(1, 80, 24, expect.any(Function)),
    )

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }))

    await waitFor(() => expect(ipc.mikrotikTerminalClose).toHaveBeenCalledWith(7))
    expect(xtermMock.state.disposed).toBe(true)
    expect(props.onDeactivated).toHaveBeenCalledWith(1)
    // Back to idle: Connect is available again.
    await waitFor(() =>
      expect(
        (screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled,
      ).toBe(false),
    )
  })

  it("shows an error banner when the open fails", async () => {
    ipc.mikrotikTerminalOpen.mockRejectedValueOnce({
      kind: "Connect",
      message: "ssh connect to router.lab: refused",
    })
    renderPanel()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))

    const banner = await screen.findByRole("alert")
    expect(banner.textContent).toContain("ssh connect to router.lab: refused")
    // A failed open stays retryable: back to idle, Connect enabled.
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    )
  })

  it("returns to idle when the backend session dies mid-write", async () => {
    const { props } = renderPanel()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(ipc.mikrotikTerminalOpen).toHaveBeenCalled())

    ipc.mikrotikTerminalWrite.mockRejectedValueOnce(new Error("NoActiveSession"))
    act(() => xtermMock.state.dataHandler?.("x"))

    const banner = await screen.findByRole("alert")
    expect(banner.textContent).toContain("ended unexpectedly")
    // Session death keeps the panel mounted (banner visible) — only a
    // deliberate disconnect deactivates the slot.
    expect(props.onDeactivated).not.toHaveBeenCalled()
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Connect" }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    )
  })
})
