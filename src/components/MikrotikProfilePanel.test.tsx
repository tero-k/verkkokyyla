// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { MikrotikProfile } from "../lib/types"
import { MikrotikProfilePanel } from "./MikrotikProfilePanel"

const ipc = vi.hoisted(() => ({
  mikrotikListProfiles: vi.fn<() => Promise<readonly MikrotikProfile[]>>(),
  mikrotikCreateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
  mikrotikUpdateProfile: vi.fn<() => Promise<MikrotikProfile>>(),
  mikrotikDeleteProfile: vi.fn<() => Promise<{ readonly deleted: boolean; readonly secretDeleted: boolean; readonly warning: string | null }>>(),
  mikrotikSetProfilePassword: vi.fn<() => Promise<void>>(),
  mikrotikTestConnection: vi.fn<() => Promise<{ readonly boardName: string | null; readonly routerosVersion: string | null; readonly architectureName: string | null }>>(),
}))

vi.mock("../lib/ipc", () => ipc)

const profiles: readonly MikrotikProfile[] = [
  { id: 7, name: "edge", host: "192.0.2.1", port: 443, useTls: true, allowInvalidCerts: false, username: "admin", hasPassword: true, createdAt: "2026-09-06T12:00:00Z" },
  { id: 8, name: "lab", host: "192.0.2.2", port: 80, useTls: false, allowInvalidCerts: true, username: "ops", hasPassword: false, createdAt: "2026-09-06T12:00:00Z" },
]

beforeEach(() => {
  vi.clearAllMocks()
  ipc.mikrotikListProfiles.mockResolvedValue(profiles)
  ipc.mikrotikUpdateProfile.mockResolvedValue(profiles[0])
  ipc.mikrotikCreateProfile.mockResolvedValue(profiles[1])
  ipc.mikrotikDeleteProfile.mockResolvedValue({ deleted: true, secretDeleted: true, warning: null })
  ipc.mikrotikSetProfilePassword.mockResolvedValue()
  ipc.mikrotikTestConnection.mockResolvedValue({ boardName: "RB5009", routerosVersion: "7.17", architectureName: "arm64" })
})

afterEach(cleanup)

function renderPanel(activeProfileId: number | null = null, onProfilesChanged?: () => void): void {
  render(<MikrotikProfilePanel activeProfileId={activeProfileId} onProfilesChanged={onProfilesChanged} />)
}

describe("MikrotikProfilePanel", () => {
  it("updates a profile without touching the keyring when password is blank", async () => {
    renderPanel()
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())

    fireEvent.click(screen.getByRole("button", { name: "Edit edge" }))
    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "edge-renamed" } })
    fireEvent.click(screen.getByRole("button", { name: "Save profile" }))

    await waitFor(() => expect(ipc.mikrotikUpdateProfile).toHaveBeenCalled())
    expect(ipc.mikrotikSetProfilePassword).not.toHaveBeenCalled()
  })

  it("confirms inactive profile deletion and refreshes the list", async () => {
    ipc.mikrotikListProfiles.mockResolvedValueOnce(profiles).mockResolvedValueOnce([profiles[0]])
    renderPanel()
    await waitFor(() => expect(screen.getByText("lab")).toBeTruthy())

    fireEvent.click(screen.getByRole("button", { name: "Delete lab" }))
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(ipc.mikrotikDeleteProfile).toHaveBeenCalledWith(8))
    await waitFor(() => expect(screen.queryByText("lab")).toBeNull())
  })

  it("notifies the parent after creating a profile so the selector refreshes", async () => {
    const onProfilesChanged = vi.fn()
    renderPanel(null, onProfilesChanged)
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())

    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "core" } })
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.3" } })
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "admin" } })
    fireEvent.click(screen.getByRole("button", { name: "Save profile" }))

    await waitFor(() => expect(ipc.mikrotikCreateProfile).toHaveBeenCalled())
    await waitFor(() => expect(onProfilesChanged).toHaveBeenCalledTimes(1))
  })

  it("notifies the parent after deleting a profile", async () => {
    const onProfilesChanged = vi.fn()
    renderPanel(null, onProfilesChanged)
    await waitFor(() => expect(screen.getByText("lab")).toBeTruthy())

    fireEvent.click(screen.getByRole("button", { name: "Delete lab" }))
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(onProfilesChanged).toHaveBeenCalledTimes(1))
  })

  it("does not notify the parent when saving fails", async () => {
    const onProfilesChanged = vi.fn()
    ipc.mikrotikCreateProfile.mockRejectedValueOnce(new Error("duplicate name"))
    renderPanel(null, onProfilesChanged)
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())

    fireEvent.change(screen.getByLabelText("Profile name"), { target: { value: "core" } })
    fireEvent.click(screen.getByRole("button", { name: "Save profile" }))

    await waitFor(() => expect(screen.getByText("duplicate name")).toBeTruthy())
    expect(onProfilesChanged).not.toHaveBeenCalled()
  })

  it("disables active profile delete and surfaces forced ProfileInUse errors", async () => {
    ipc.mikrotikDeleteProfile.mockRejectedValueOnce({ kind: "ProfileInUse", message: "profile backs active session" })
    renderPanel(7)
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())

    const activeRow = screen.getByTestId("profile-row-7")
    const deleteButton = within(activeRow).getByRole("button", { name: "Delete edge" })
    expect(deleteButton.hasAttribute("disabled")).toBe(true)
    expect(within(activeRow).getByTitle("Cannot delete the profile backing the active session")).toBeTruthy()

    cleanup()
    renderPanel()
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())
    fireEvent.click(screen.getByRole("button", { name: "Delete edge" }))
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(screen.getByText("profile backs active session")).toBeTruthy())
  })

  it("shows typed 401 test connection errors", async () => {
    ipc.mikrotikTestConnection.mockRejectedValueOnce({ kind: "unauthorized", message: "authentication failed (401): check username and password" })
    renderPanel()
    await waitFor(() => expect(screen.getByText("edge")).toBeTruthy())

    fireEvent.click(screen.getByRole("button", { name: "Test connection for edge" }))

    await waitFor(() => expect(screen.getByText("authentication failed (401): check username and password")).toBeTruthy())
  })
})
