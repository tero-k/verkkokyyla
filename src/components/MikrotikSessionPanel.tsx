import type { MikrotikSessionSummaryDto } from "../lib/types"

import styles from "./TraceSessionPanel.module.css"

type MikrotikSessionPanelProps = {
  readonly sessions: readonly MikrotikSessionSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

function formatEnded(endedAt: string | null): string {
  return endedAt === null ? "running" : formatDateTime(endedAt)
}

function formatDevice(session: MikrotikSessionSummaryDto): string {
  const board = session.boardName ?? "unknown board"
  const routeros = session.routerosVersion ?? "unknown RouterOS"
  return `${board} / ${routeros}`
}

export function MikrotikSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
}: MikrotikSessionPanelProps) {
  return (
    <div className={styles.wrapper} data-testid="mikrotik-session-panel">
      <h2>MikroTik history</h2>
      {sessions.length === 0 ? (
        <p className={styles.empty}>No saved MikroTik sessions yet.</p>
      ) : (
        <ul className={styles.list}>
          {sessions.map((session) => (
            <li
              key={session.id}
              className={styles.item}
              data-testid="mikrotik-session-item"
            >
              <div className={styles.summary}>
                <span className={styles.target}>{formatDevice(session)}</span>
                <span className={styles.meta}>
                  {formatDateTime(session.startedAt)} - {formatEnded(session.endedAt)}
                </span>
                <span className={styles.meta}>
                  {session.snapshotCount} snapshots - {session.status}
                </span>
              </div>
              <div className={styles.actions}>
                <button
                  type="button"
                  onClick={() => onOpen(session.id)}
                  disabled={disabled}
                  data-testid="mikrotik-open-session"
                >
                  Load
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(session.id)}
                  disabled={disabled}
                  data-testid="mikrotik-delete-session"
                >
                  Delete
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
