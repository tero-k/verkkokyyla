import { useCallback, useEffect, useState } from "react"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { formatMetric } from "../lib/format"
import { mikrotikDeleteBackup, mikrotikListBackups } from "../lib/ipc"
import type { MikrotikBackupRecordDto } from "../lib/types"

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

export function MikrotikBackupLibrary({ refreshKey }: MikrotikBackupLibraryProps) {
  const [backups, setBackups] = useState<readonly MikrotikBackupRecordDto[]>([])
  const [loading, setLoading] = useState(true)
  const [deletingId, setDeletingId] = useState<number | null>(null)
  const [error, setError] = useState("")
  const [warnings, setWarnings] = useState<readonly string[]>([])
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

  return (
    <section
      className={styles.panel}
      aria-label="MikroTik backup library"
      data-testid="mikrotik-backup-library"
    >
      <header className={styles.header}>
        <h2>Backup library</h2>
        <p>Local backup history from all MikroTik profiles.</p>
      </header>

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
              className={styles.item}
              data-testid={`mikrotik-backup-row-${backup.id}`}
              key={backup.id}
            >
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
      {dialog}
    </section>
  )
}
