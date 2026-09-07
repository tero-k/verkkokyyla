// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { MikrotikFirmwareStatus, MikrotikUpdateStatus } from "../lib/types"
import { MikrotikVersionPanel } from "./MikrotikVersionPanel"

const ipc = vi.hoisted(() => ({
  mikrotikCheckUpdates: vi.fn<() => Promise<{ readonly updateStatus: MikrotikUpdateStatus; readonly firmwareStatus: MikrotikFirmwareStatus }>>(),
  mikrotikFetchChangelog: vi.fn<() => Promise<{ readonly version: string; readonly changelog: string }>>(),
}))

vi.mock("../lib/ipc", () => ipc)

const unknownUpdate: MikrotikUpdateStatus = {
  installedVersion: "7.15.3",
  latestVersion: null,
  channel: "stable",
  state: "unknown",
  status: "unknown",
}

const notApplicableFirmware: MikrotikFirmwareStatus = {
  state: "not-applicable",
  currentFirmware: null,
  upgradeFirmware: null,
  model: null,
}

afterEach(cleanup)

beforeEach(() => {
  vi.clearAllMocks()
  ipc.mikrotikCheckUpdates.mockResolvedValue({
    updateStatus: {
      installedVersion: "7.15.3",
      latestVersion: "7.17",
      channel: "stable",
      state: "update-available",
      status: "new-version-available",
    },
    firmwareStatus: { state: "available", currentFirmware: "7.15.3", upgradeFirmware: "7.17", model: "RB5009" },
  })
  ipc.mikrotikFetchChangelog.mockResolvedValue({ version: "7.17", changelog: "fixed routing\nupdated wireless" })
})

describe("MikrotikVersionPanel", () => {
  it("renders unknown latest version and not-applicable firmware states", () => {
    render(<MikrotikVersionPanel profileId={7} updateStatus={unknownUpdate} firmwareStatus={notApplicableFirmware} />)

    expect(screen.getByTestId("routeros-latest").textContent).toBe("unknown")
    expect(screen.getByTestId("routeros-badge").textContent).toBe("unknown")
    expect(screen.getByTestId("firmware-badge").textContent).toBe("Not applicable")
  })

  it("handles empty health state without crashing", () => {
    render(<MikrotikVersionPanel profileId={null} updateStatus={null} firmwareStatus={null} />)

    expect(screen.getByText("RouterOS")).toBeTruthy()
    expect(screen.getAllByText("unknown").length).toBeGreaterThan(0)
  })

  it("checks updates and renders the returned status", async () => {
    render(<MikrotikVersionPanel profileId={7} updateStatus={null} firmwareStatus={null} />)

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }))

    await waitFor(() => expect(ipc.mikrotikCheckUpdates).toHaveBeenCalledWith(7))
    expect(screen.getByTestId("routeros-badge").textContent).toBe("update available")
    expect(screen.getByTestId("firmware-badge").textContent).toBe("upgrade available")
  })

  it("uses canonical state fields for up-to-date badges", () => {
    render(
      <MikrotikVersionPanel
        profileId={7}
        updateStatus={{ ...unknownUpdate, latestVersion: "7.17", state: "up-to-date", status: "System is already up to date" }}
        firmwareStatus={{ state: "up-to-date", currentFirmware: "7.17", upgradeFirmware: "7.17", model: "RB5009" }}
      />,
    )

    expect(screen.getByTestId("routeros-badge").textContent).toBe("up to date")
    expect(screen.getByTestId("firmware-badge").textContent).toBe("up to date")
  })

  it("fetches changelog text into a monospace panel", async () => {
    render(<MikrotikVersionPanel profileId={7} updateStatus={{ ...unknownUpdate, latestVersion: "7.17" }} firmwareStatus={notApplicableFirmware} />)

    fireEvent.click(screen.getByRole("button", { name: "View changelog" }))

    await waitFor(() => expect(ipc.mikrotikFetchChangelog).toHaveBeenCalledWith("7.17"))
    expect(screen.getByTestId("mikrotik-changelog").textContent).toContain("fixed routing")
    expect(screen.getByTestId("mikrotik-changelog").className).toMatch(/changelog/)
  })

  it("renders an inline muted changelog fetch error", async () => {
    ipc.mikrotikFetchChangelog.mockRejectedValue({ kind: "network", message: "changelog offline" })
    render(<MikrotikVersionPanel profileId={7} updateStatus={{ ...unknownUpdate, latestVersion: "7.17" }} firmwareStatus={notApplicableFirmware} />)

    fireEvent.click(screen.getByRole("button", { name: "View changelog" }))

    await waitFor(() => expect(screen.getByText("changelog offline").className).toMatch(/mutedError/))
  })
})
