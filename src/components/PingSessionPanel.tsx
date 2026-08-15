import type { SessionSummaryDto } from "../lib/types"

import styles from "./PingSessionPanel.module.css"

type PingSessionPanelProps = {
  readonly sessions: readonly SessionSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

export function PingSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
}: PingSessionPanelProps) {
  return (
    <div
      className={`${styles.wrapper}${disabled ? ` ${styles.disabled}` : ""}`}
      data-testid="session-panel"
    >
      <h2>Past sessions</h2>
      {sessions.length === 0 ? (
        <p className={styles.empty}>No saved sessions yet.</p>
      ) : (
        <ul className={styles.list}>
          {sessions.map((session) => (
            <li key={session.id} className={styles.item} data-testid="session-item">
              <div className={styles.summary}>
                <span className={styles.target}>{session.targetInput}</span>
                <span className={styles.meta}>
                  {session.resolvedIp} · {session.family} · {formatDateTime(session.startedAt)}
                </span>
                <span className={styles.meta}>
                  {session.probeCount} probes · {session.lossPercent.toFixed(2)}% loss
                </span>
              </div>
              <div className={styles.actions}>
                <button
                  type="button"
                  onClick={() => onOpen(session.id)}
                  disabled={disabled}
                  data-testid="session-open"
                >
                  Reopen
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(session.id)}
                  disabled={disabled}
                  data-testid="session-delete"
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
