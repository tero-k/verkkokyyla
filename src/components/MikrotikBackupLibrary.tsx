import { useCallback, useEffect, useMemo, useState } from "react"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { formatMetric } from "../lib/format"
import { mikrotikDeleteBackup, mikrotikDiffBackups, mikrotikListBackups } from "../lib/ipc"
import type { BackupDiffDto, BackupDiffLineDto, MikrotikBackupRecordDto } from "../lib/types"
import { Button, SectionHeader } from "./ui/ui"

import styles from "./MikrotikBackupLibrary.module.css"

type MikrotikBackupLibraryProps = {
  readonly refreshKey: number
}

function messageFrom(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "object" && error !== null && "message" in error) {
    return String(error.message)
  }
  return String(error)
}

function formatDateTime(createdAt: string): string {
  const date = new Date(createdAt)
  return Number.isNaN(date.getTime()) ? "-" : date.toLocaleString()
}

function formatBytes(value: number): string {
  if (value >= 1_073_741_824) return `${formatMetric(value / 1_073_741_824)} GB`
  if (value >= 1_048_576) return `${formatMetric(value / 1_048_576)} MB`
  if (value >= 1_024) return `${formatMetric(value / 1_024)} KB`
  return `${formatMetric(value, 0)} B`
}

/** Collapse long runs of unchanged lines into a placeholder row. */
type DiffRow =
  | { readonly kind: "line"; readonly line: BackupDiffLineDto }
  | { readonly kind: "gap"; readonly groupId: number; readonly lines: readonly BackupDiffLineDto[] }

const CONTEXT = 3

function collapseDiff(lines: readonly BackupDiffLineDto[]): DiffRow[] {
  const rows: DiffRow[] = []
  let sameStart = -1
  let groupId = 0

  const flush = (end: number, endIsChange: boolean) => {
    if (sameStart < 0) return
    const run = lines.slice(sameStart, end)
    const headCount = sameStart === 0 ? 0 : CONTEXT
    const tailCount = endIsChange ? CONTEXT : 0
    if (run.length <= headCount + tailCount + 1) {
      // Too short to bother collapsing.
      for (const line of run) rows.push({ kind: "line", line })
    } else {
      for (const line of run.slice(0, headCount)) rows.push({ kind: "line", line })
      const hidden = run.slice(headCount, run.length - tailCount)
      groupId += 1
      rows.push({ kind: "gap", groupId, lines: hidden })
      for (const line of run.slice(run.length - tailCount)) {
        rows.push({ kind: "line", line })
      }
    }
    sameStart = -1
  }

  lines.forEach((line, index) => {
    if (line.kind === "same") {
      if (sameStart < 0) sameStart = index
    } else {
      flush(index, true)
      rows.push({ kind: "line", line })
    }
  })
  flush(lines.length, false)
  return rows
}

function DiffLine({ line }: { readonly line: BackupDiffLineDto }) {
  const className = `${styles.diffLine} ${
    line.kind === "add"
      ? styles.diffAdd
      : line.kind === "remove"
        ? styles.diffRemove
        : ""
  }`
  return (
    <div className={className} data-testid={`mikrotik-backup-diff-${line.kind}`}>
      <span className={styles.diffSign}>
        {line.kind === "add" ? "+" : line.kind === "remove" ? "−" : " "}
      </span>
      <span className={styles.diffText}>{line.text}</span>
    </div>
  )
}

export function MikrotikBackupLibrary({ refreshKey }: MikrotikBackupLibraryProps) {
  const [backups, setBackups] = useState<readonly MikrotikBackupRecordDto[]>([])
  const [loading, setLoading] = useState(true)
  const [deletingId, setDeletingId] = useState<number | null>(null)
  const [error, setError] = useState("")
  const [warnings, setWarnings] = useState<readonly string[]>([])
  const [compareSelection, setCompareSelection] = useState<readonly number[]>([])
  const [diff, setDiff] = useState<BackupDiffDto | null>(null)
  const [diffBusy, setDiffBusy] = useState(false)
  const [expandedGaps, setExpandedGaps] = useState<readonly number[]>([])
  const { confirm, dialog } = useConfirmDialog()

  const loadBackups = useCallback(async (): Promise<void> => {
    setLoading(true)
    setError("")
    try {
      setBackups(await mikrotikListBackups())
    } catch (caught) {
      setError(messageFrom(caught))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void loadBackups()
  }, [loadBackups, refreshKey])

  // Drop selections that no longer exist after a reload.
  useEffect(() => {
    setCompareSelection((current) => {
      const ids = new Set(backups.map((backup) => backup.id))
      const next = current.filter((id) => ids.has(id))
      return next.length === current.length ? current : next
    })
  }, [backups])

  async function remove(backup: MikrotikBackupRecordDto): Promise<void> {
    const confirmed = await confirm(
      `Delete backup ${backup.name}? This removes the saved file(s) from disk.`,
    )
    if (!confirmed) return

    setDeletingId(backup.id)
    setError("")
    setWarnings([])
    try {
      const result = await mikrotikDeleteBackup(backup.id)
      setWarnings(result.warnings)
      await loadBackups()
    } catch (caught) {
      setError(messageFrom(caught))
    } finally {
      setDeletingId(null)
    }
  }

  function toggleCompare(id: number): void {
    setDiff(null)
    setCompareSelection((current) => {
      if (current.includes(id)) return current.filter((item) => item !== id)
      return [...current, id].slice(-2)
    })
  }

  const comparable = useMemo(
    () => backups.filter((backup) => backup.hasRscExport),
    [backups],
  )
  const canCompare = compareSelection.length === 2

  async function runCompare(): Promise<void> {
    if (!canCompare) return
    const [a, b] = compareSelection
    const first = backups.find((backup) => backup.id === a)
    const second = backups.find((backup) => backup.id === b)
    if (!first || !second) return
    // Older = earlier createdAt (fall back to lower id when unparseable).
    const firstTime = Date.parse(first.createdAt)
    const secondTime = Date.parse(second.createdAt)
    const firstOlder = Number.isNaN(firstTime) || Number.isNaN(secondTime)
      ? first.id < second.id
      : firstTime <= secondTime
    const older = firstOlder ? first : second
    const newer = firstOlder ? second : first

    setDiffBusy(true)
    setError("")
    setExpandedGaps([])
    try {
      setDiff(await mikrotikDiffBackups(older.id, newer.id))
    } catch (caught) {
      setDiff(null)
      setError(messageFrom(caught))
    } finally {
      setDiffBusy(false)
    }
  }

  const diffRows = useMemo(() => (diff ? collapseDiff(diff.lines) : []), [diff])

  return (
    <section
      className={styles.panel}
      aria-label="MikroTik backup library"
      data-testid="mikrotik-backup-library"
    >
      <div className={styles.headerRow}>
        <header className={styles.header}>
          <h2>Backup library</h2>
          <p>Local backup history from all MikroTik profiles.</p>
        </header>
        <Button
          small
          variant="primary"
          disabled={!canCompare || diffBusy}
          onClick={() => void runCompare()}
          data-testid="mikrotik-backup-compare"
        >
          {diffBusy ? "Comparing..." : "Compare selected"}
        </Button>
      </div>
      {backups.length > 0 && comparable.length < 2 ? (
        <div className={styles.notice} role="status" data-testid="mikrotik-backup-diff-notice">
          <p>
            Config diff needs two backups with .rsc exports. Backups saved without the
            export (no “+ .rsc” badge) cannot be compared — their checkboxes are
            disabled. Create new backups with “Include .rsc export” enabled to diff them.
          </p>
        </div>
      ) : null}
      {comparable.length >= 2 ? (
        <p className={styles.compareHint}>
          Select two backups with an .rsc export to compare their configs.
        </p>
      ) : null}

      {error ? <p className={styles.error} role="alert">{error}</p> : null}
      {warnings.length > 0 ? (
        <div className={styles.notice} role="status">
          <p>Backup record deleted with warnings:</p>
          <ul>{warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
        </div>
      ) : null}
      {loading ? <p className={styles.state} role="status">Loading backups...</p> : null}
      {!loading && backups.length === 0 ? (
        <p className={styles.state}>No MikroTik backups saved yet.</p>
      ) : null}
      {!loading && backups.length > 0 ? (
        <ul className={styles.list}>
          {backups.map((backup) => (
            <li
              className={`${styles.item}${compareSelection.includes(backup.id) ? ` ${styles.itemSelected}` : ""}`}
              data-testid={`mikrotik-backup-row-${backup.id}`}
              key={backup.id}
            >
              <input
                type="checkbox"
                className={styles.compareCheck}
                aria-label={backup.hasRscExport
                  ? `Select ${backup.name} for comparison`
                  : `${backup.name} has no .rsc export and cannot be compared`}
                title={backup.hasRscExport
                  ? "Select for comparison"
                  : "No .rsc export — cannot be compared. Create backups with “Include .rsc export” to diff them."}
                checked={compareSelection.includes(backup.id)}
                onChange={() => toggleCompare(backup.id)}
                disabled={diffBusy || !backup.hasRscExport}
              />
              <div className={styles.summary}>
                <div className={styles.titleRow}>
                  <strong>{backup.name}</strong>
                  {backup.hasRscExport ? <span className={styles.badge}>+ .rsc</span> : null}
                </div>
                <div className={styles.metadata}>
                  <span>{backup.profileName}</span>
                  <span>{formatDateTime(backup.createdAt)}</span>
                  <span>{formatBytes(backup.sizeBytes)}</span>
                </div>
                <p className={styles.path}>{backup.backupPath}</p>
                {backup.exportPath ? <p className={styles.path}>{backup.exportPath}</p> : null}
              </div>
              <button
                className={styles.deleteButton}
                type="button"
                aria-label={`Delete ${backup.name}`}
                disabled={deletingId !== null}
                onClick={() => void remove(backup)}
              >
                {deletingId === backup.id ? "Deleting..." : "Delete"}
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      {diff ? (
        <div className={styles.diffPanel} data-testid="mikrotik-backup-diff">
          <SectionHeader
            title={`Config diff: ${diff.olderName} → ${diff.newerName}`}
            aside={`+${diff.added} −${diff.removed}`}
          />
          {diff.added === 0 && diff.removed === 0 ? (
            <p className={styles.state}>No config changes between these backups.</p>
          ) : (
            <div className={styles.diffBody} role="table" aria-label="Config diff">
              {diffRows.flatMap((row, index) => {
                if (row.kind === "gap") {
                  if (expandedGaps.includes(row.groupId)) {
                    return row.lines.map((line, gapIndex) => (
                      <DiffLine key={`gap-${row.groupId}-${gapIndex}`} line={line} />
                    ))
                  }
                  return [
                    <button
                      key={`gap-${row.groupId}`}
                      type="button"
                      className={styles.diffGap}
                      onClick={() =>
                        setExpandedGaps((current) => [...current, row.groupId])
                      }
                    >
                      ··· {row.lines.length} unchanged lines — click to expand ···
                    </button>,
                  ]
                }
                return [<DiffLine key={`line-${index}`} line={row.line} />]
              })}
            </div>
          )}
        </div>
      ) : null}
      {dialog}
    </section>
  )
}
