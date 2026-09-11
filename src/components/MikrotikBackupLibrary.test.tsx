// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { DeleteMikrotikBackupResultDto, MikrotikBackupRecordDto } from "../lib/types"
import { MikrotikBackupLibrary } from "./MikrotikBackupLibrary"

const ipc = vi.hoisted(() => ({
  mikrotikDeleteBackup: vi.fn<(id: number) => Promise<DeleteMikrotikBackupResultDto>>(),
  mikrotikListBackups: vi.fn<() => Promise<MikrotikBackupRecordDto[]>>(),
}))

vi.mock("../lib/ipc", () => ipc)

const backups: readonly MikrotikBackupRecordDto[] = [
  {
    id: 12,
    profileId: 7,
    profileName: "edge-router",
    name: "edge-nightly",
    backupPath: "C:/backups/edge-nightly.backup",
    exportPath: "C:/backups/edge-nightly.rsc",
    createdAt: "2026-09-10T18:30:00Z",
    sizeBytes: 1_572_864,
    hasRscExport: true,
  },
  {
    id: 11,
    profileId: null,
    profileName: "Deleted profile",
    name: "legacy",
    backupPath: "C:/backups/legacy.backup",
    exportPath: null,
    createdAt: "not-a-date",
    sizeBytes: 2_048,
    hasRscExport: false,
  },
]

beforeEach(() => {
  vi.clearAllMocks()
  ipc.mikrotikListBackups.mockResolvedValue([...backups])
  ipc.mikrotikDeleteBackup.mockResolvedValue({ deleted: true, warnings: [] })
})

afterEach(cleanup)

describe("MikrotikBackupLibrary", () => {
  it("loads and renders backup metadata", async () => {
    render(<MikrotikBackupLibrary refreshKey={0} />)

    await waitFor(() => expect(screen.getByText("edge-nightly")).toBeTruthy())

    expect(screen.getByText("edge-router")).toBeTruthy()
    expect(screen.getByText("1.50 MB")).toBeTruthy()
    expect(screen.getByText("+ .rsc")).toBeTruthy()
    expect(screen.getByText("C:/backups/edge-nightly.backup")).toBeTruthy()
    expect(screen.getByText("C:/backups/edge-nightly.rsc")).toBeTruthy()
    expect(screen.getByText("-")).toBeTruthy()
  })

  it("renders the empty state when no backups exist", async () => {
    ipc.mikrotikListBackups.mockResolvedValue([])

    render(<MikrotikBackupLibrary refreshKey={0} />)

    await waitFor(() => expect(screen.getByText("No MikroTik backups saved yet.")).toBeTruthy())
  })

  it("confirms deletion, deletes the record, and reloads", async () => {
    ipc.mikrotikListBackups.mockResolvedValueOnce([...backups]).mockResolvedValueOnce([backups[1]])
    render(<MikrotikBackupLibrary refreshKey={0} />)
    const row = await screen.findByTestId("mikrotik-backup-row-12")

    fireEvent.click(within(row).getByRole("button", { name: "Delete edge-nightly" }))
    expect(screen.getByText("Delete backup edge-nightly? This removes the saved file(s) from disk.")).toBeTruthy()
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(ipc.mikrotikDeleteBackup).toHaveBeenCalledWith(12))
    await waitFor(() => expect(ipc.mikrotikListBackups).toHaveBeenCalledTimes(2))
    expect(screen.queryByText("edge-nightly")).toBeNull()
  })

  it("shows file deletion warnings after reloading", async () => {
    ipc.mikrotikDeleteBackup.mockResolvedValue({
      deleted: true,
      warnings: ["Export file was already missing"],
    })
    render(<MikrotikBackupLibrary refreshKey={0} />)
    const row = await screen.findByTestId("mikrotik-backup-row-12")

    fireEvent.click(within(row).getByRole("button", { name: "Delete edge-nightly" }))
    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"))

    await waitFor(() => expect(screen.getByText("Export file was already missing")).toBeTruthy())
  })

  it("reloads when refreshKey changes", async () => {
    const { rerender } = render(<MikrotikBackupLibrary refreshKey={0} />)
    await waitFor(() => expect(ipc.mikrotikListBackups).toHaveBeenCalledTimes(1))

    rerender(<MikrotikBackupLibrary refreshKey={1} />)

    await waitFor(() => expect(ipc.mikrotikListBackups).toHaveBeenCalledTimes(2))
  })
})
