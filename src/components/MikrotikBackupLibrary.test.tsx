// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { BackupDiffDto, DeleteMikrotikBackupResultDto, MikrotikBackupRecordDto } from "../lib/types"
import { MikrotikBackupLibrary } from "./MikrotikBackupLibrary"

const ipc = vi.hoisted(() => ({
  mikrotikDeleteBackup: vi.fn<(id: number) => Promise<DeleteMikrotikBackupResultDto>>(),
  mikrotikListBackups: vi.fn<() => Promise<MikrotikBackupRecordDto[]>>(),
  mikrotikDiffBackups: vi.fn<(olderId: number, newerId: number) => Promise<BackupDiffDto>>(),
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

  describe("config diff", () => {
    const secondExportable: MikrotikBackupRecordDto = {
      id: 13,
      profileId: 7,
      profileName: "edge-router",
      name: "edge-morning",
      backupPath: "C:/backups/edge-morning.backup",
      exportPath: "C:/backups/edge-morning.rsc",
      createdAt: "2026-09-11T03:15:00Z",
      sizeBytes: 1_572_900,
      hasRscExport: true,
    }

    const diffResult: BackupDiffDto = {
      olderId: 12,
      olderName: "edge-nightly",
      olderCreatedAt: "2026-09-10T18:30:00Z",
      newerId: 13,
      newerName: "edge-morning",
      newerCreatedAt: "2026-09-11T03:15:00Z",
      lines: [
        ...Array.from({ length: 10 }, (_, i): BackupDiffDto["lines"][number] => ({
          kind: "same",
          text: `unchanged line ${i + 1}`,
        })),
        { kind: "remove", text: "/ip dhcp-server lease remove 10.0.0.9" },
        { kind: "add", text: "/ip firewall filter add chain=forward" },
      ],
      added: 1,
      removed: 1,
    }

    it("keeps Compare disabled until two .rsc backups are selected", async () => {
      ipc.mikrotikListBackups.mockResolvedValue([...backups, secondExportable])
      render(<MikrotikBackupLibrary refreshKey={0} />)
      await screen.findByTestId("mikrotik-backup-row-12")

      const compare = screen.getByTestId("mikrotik-backup-compare")
      expect(compare.hasAttribute("disabled")).toBe(true)

      // The row without an .rsc export shows a disabled checkbox with a reason.
      const legacy = screen.getByLabelText("legacy has no .rsc export and cannot be compared")
      expect(legacy.hasAttribute("disabled")).toBe(true)

      fireEvent.click(screen.getByLabelText("Select edge-nightly for comparison"))
      expect(compare.hasAttribute("disabled")).toBe(true)
      fireEvent.click(screen.getByLabelText("Select edge-morning for comparison"))
      expect(compare.hasAttribute("disabled")).toBe(false)
    })

    it("diffs the two selected backups older-first and renders changes", async () => {
      ipc.mikrotikListBackups.mockResolvedValue([secondExportable, ...backups])
      ipc.mikrotikDiffBackups.mockResolvedValue(diffResult)
      render(<MikrotikBackupLibrary refreshKey={0} />)
      await screen.findByTestId("mikrotik-backup-row-12")

      // Select newest first to prove ordering comes from timestamps, not clicks.
      fireEvent.click(screen.getByLabelText("Select edge-morning for comparison"))
      fireEvent.click(screen.getByLabelText("Select edge-nightly for comparison"))
      fireEvent.click(screen.getByTestId("mikrotik-backup-compare"))

      await waitFor(() => expect(ipc.mikrotikDiffBackups).toHaveBeenCalledWith(12, 13))
      const diff = await screen.findByTestId("mikrotik-backup-diff")
      expect(within(diff).getByText("Config diff: edge-nightly → edge-morning")).toBeTruthy()
      expect(within(diff).getByText("+1 −1")).toBeTruthy()
      expect(within(diff).getByTestId("mikrotik-backup-diff-add")).toBeTruthy()
      expect(within(diff).getByTestId("mikrotik-backup-diff-remove")).toBeTruthy()
    })

    it("collapses long unchanged runs and expands them on click", async () => {
      ipc.mikrotikListBackups.mockResolvedValue([...backups, secondExportable])
      ipc.mikrotikDiffBackups.mockResolvedValue(diffResult)
      render(<MikrotikBackupLibrary refreshKey={0} />)
      await screen.findByTestId("mikrotik-backup-row-12")

      fireEvent.click(screen.getByLabelText("Select edge-nightly for comparison"))
      fireEvent.click(screen.getByLabelText("Select edge-morning for comparison"))
      fireEvent.click(screen.getByTestId("mikrotik-backup-compare"))

      const gap = await screen.findByText(/unchanged lines/)
      expect(screen.queryByText("unchanged line 5")).toBeNull()
      fireEvent.click(gap)
      await waitFor(() => expect(screen.getByText("unchanged line 5")).toBeTruthy())
    })
  })
})
