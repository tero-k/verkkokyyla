import type { TraceSummaryDto } from "../lib/types"

import styles from "./TraceSessionPanel.module.css"

type TraceSessionPanelProps = {
  readonly sessions: readonly TraceSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

function formatReached(reached: boolean): string {
  return reached ? "Yes" : "No"
}

export function TraceSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
}: TraceSessionPanelProps) {
  return (
    <div className={styles.wrapper} data-testid="trace-session-panel">
      <h2>Trace history</h2>
      {sessions.length === 0 ? (
        <p className={styles.empty}>No saved traces yet.</p>
      ) : (
        <ul className={styles.list}>
          {sessions.map((session) => (
            <li key={session.id} className={styles.item} data-testid="trace-session-item">
              <div className={styles.summary}>
                <span className={styles.target}>{session.targetInput}</span>
                <span className={styles.meta}>
                  {session.resolvedIp} · {formatDateTime(session.startedAt)}
                </span>
                <span className={styles.meta}>
                  {session.hopCount} hops · reached {formatReached(session.reachedTarget)}
                </span>
                <span className={styles.meta}>{session.status}</span>
              </div>
              <div className={styles.actions}>
                <button
                  type="button"
                  onClick={() => onOpen(session.id)}
                  disabled={disabled}
                  data-testid="trace-open"
                >
                  Reopen
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(session.id)}
                  disabled={disabled}
                  data-testid="trace-delete"
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
