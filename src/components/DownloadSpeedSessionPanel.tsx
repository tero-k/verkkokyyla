import type { DownloadSpeedSessionSummaryDto } from "../lib/types"

import styles from "./DownloadSpeedSessionPanel.module.css"

type DownloadSpeedSessionPanelProps = {
  readonly sessions: readonly DownloadSpeedSessionSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

function formatMbps(mbps: number): string {
  if (!Number.isFinite(mbps)) return "-"
  return `${mbps.toFixed(2)} Mbps`
}

export function DownloadSpeedSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
}: DownloadSpeedSessionPanelProps) {
  return (
    <div className={styles.wrapper} data-testid="download-speed-session-panel">
      <h2>Speed test history</h2>
      {sessions.length === 0 ? (
        <p className={styles.empty}>No saved speed tests yet.</p>
      ) : (
        <ul className={styles.list}>
          {sessions.map((session) => (
            <li key={session.id} className={styles.item} data-testid="download-speed-session-item">
              <div className={styles.summary}>
                <span className={styles.target}>{session.url}</span>
                <span className={styles.meta}>
                  {session.mode === "page" ? "Full page" : "Single file"} ·{" "}
                  {formatDateTime(session.startedAt)}
                </span>
                <span className={styles.meta}>
                  {formatMbps(session.averageMbps)} · {session.totalTimeMs} ms
                </span>
              </div>
              <div className={styles.actions}>
                <button
                  type="button"
                  onClick={() => onOpen(session.id)}
                  disabled={disabled}
                  data-testid="download-speed-open"
                >
                  Open
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(session.id)}
                  disabled={disabled}
                  data-testid="download-speed-delete"
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
