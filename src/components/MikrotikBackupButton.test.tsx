// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { MikrotikBackupButton } from "./MikrotikBackupButton"

const ipc = vi.hoisted(() => ({
  mikrotikBackup: vi.fn<() => Promise<{ readonly backupPath: string; readonly exportPath: string | null; readonly cleanupWarnings: readonly string[] }>>(),
}))

const dialog = vi.hoisted(() => ({
  open: vi.fn<() => Promise<string | null>>(),
}))

vi.mock("../lib/ipc", () => ipc)
vi.mock("@tauri-apps/plugin-dialog", () => dialog)

const reservedNames = [
  "CON", "PRN", "AUX", "NUL",
  "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
  "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
  "con", "Com1", "lPt9", "NUL.backup", "COM1.rsc", "LPT2.txt",
] as const

function openDialog(): void {
  render(<MikrotikBackupButton profileId={7} />)
  fireEvent.click(screen.getByRole("button", { name: "Backup" }))
}

function chooseDirectory(): void {
  fireEvent.click(screen.getByRole("button", { name: "Choose directory" }))
}

afterEach(cleanup)

beforeEach(() => {
  vi.clearAllMocks()
  dialog.open.mockResolvedValue("C:/backups")
  ipc.mikrotikBackup.mockResolvedValue({
    backupPath: "C:/backups/verkkokyyla.backup",
    exportPath: "C:/backups/verkkokyyla.rsc",
    cleanupWarnings: [],
  })
})

describe("MikrotikBackupButton", () => {
  it.each(reservedNames)("disables confirm for reserved Windows device name %s", async (name) => {
    openDialog()
    const backupName = screen.getByLabelText("Backup name")

    fireEvent.change(backupName, { target: { value: name } })
    chooseDirectory()

    await waitFor(() => expect(dialog.open).toHaveBeenCalledWith({ directory: true }))
    expect(screen.getByRole("button", { name: "Create backup" }).hasAttribute("disabled")).toBe(true)
  })

  it.each(["bad/name", "bad\\name", "bad:name", " bad", ".hidden", ""])(
    "disables confirm for invalid backup name %s",
    async (name) => {
      openDialog()

      fireEvent.change(screen.getByLabelText("Backup name"), { target: { value: name } })
      chooseDirectory()

      await waitFor(() => expect(dialog.open).toHaveBeenCalled())
      expect(screen.getByRole("button", { name: "Create backup" }).hasAttribute("disabled")).toBe(true)
    },
  )

  it("retries once with overwrite after OutputExists confirmation", async () => {
    ipc.mikrotikBackup
      .mockRejectedValueOnce({ kind: "OutputExists", message: "output already exists" })
      .mockResolvedValueOnce({ backupPath: "C:/backups/lab.backup", exportPath: null, cleanupWarnings: [] })
    openDialog()

    fireEvent.change(screen.getByLabelText("Backup name"), { target: { value: "lab" } })
    chooseDirectory()
    await waitFor(() => expect(screen.getByText("C:/backups")).toBeTruthy())
    fireEvent.click(screen.getByRole("button", { name: "Create backup" }))
    await waitFor(() => expect(screen.getByTestId("confirm-dialog")).toBeTruthy())
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(ipc.mikrotikBackup).toHaveBeenCalledTimes(2))
    expect(ipc.mikrotikBackup).toHaveBeenLastCalledWith(7, "C:/backups", "lab", undefined, false, true)
    expect(screen.getByText("C:/backups/lab.backup")).toBeTruthy()
  })

  it("submits selected directory and transient encryption password", async () => {
    openDialog()

    fireEvent.change(screen.getByLabelText("Backup name"), { target: { value: "lab" } })
    fireEvent.change(screen.getByLabelText("Encryption password (optional)"), { target: { value: "secret" } })
    fireEvent.click(screen.getByLabelText("Include .rsc export"))
    chooseDirectory()
    await waitFor(() => expect(screen.getByText("C:/backups")).toBeTruthy())
    fireEvent.click(screen.getByRole("button", { name: "Create backup" }))

    await waitFor(() => expect(ipc.mikrotikBackup).toHaveBeenCalledWith(7, "C:/backups", "lab", "secret", true, false))
    expect(screen.queryByDisplayValue("secret")).toBeNull()
  })

  it("renders SSH guidance for unreachable SSH errors", async () => {
    ipc.mikrotikBackup.mockRejectedValueOnce({ kind: "SshUnreachable", message: "connection refused" })
    openDialog()

    fireEvent.change(screen.getByLabelText("Backup name"), { target: { value: "lab" } })
    chooseDirectory()
    await waitFor(() => expect(screen.getByText("C:/backups")).toBeTruthy())
    fireEvent.click(screen.getByRole("button", { name: "Create backup" }))

    await waitFor(() => expect(screen.getByText(/Enable the SSH service on the router/)).toBeTruthy())
  })
})
