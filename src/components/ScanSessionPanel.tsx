import type { ScanSummaryDto } from "../lib/types"

import styles from "./ScanSessionPanel.module.css"

type ScanSessionPanelProps = {
  readonly scans: readonly ScanSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

export function ScanSessionPanel({
  scans,
  disabled,
  onOpen,
  onDelete,
}: ScanSessionPanelProps) {
  return (
    <div className={styles.wrapper} data-testid="scan-session-panel">
      <h2>Scan history</h2>
      {scans.length === 0 ? (
        <p className={styles.empty}>No saved scans yet.</p>
      ) : (
        <ul className={styles.list}>
          {scans.map((scan) => (
            <li key={scan.id} className={styles.item} data-testid="scan-session-item">
              <div className={styles.summary}>
                <span className={styles.target}>{scan.interfaceName}</span>
                <span className={styles.meta}>
                  {scan.cidr} · {formatDateTime(scan.startedAt)}
                </span>
                <span className={styles.meta}>
                  {scan.hostCount} hosts · {scan.status}
                </span>
              </div>
              <div className={styles.actions}>
                <button
                  type="button"
                  onClick={() => onOpen(scan.id)}
                  disabled={disabled}
                  data-testid="scan-open"
                >
                  Open
                </button>
                <button
                  type="button"
                  onClick={() => onDelete(scan.id)}
                  disabled={disabled}
                  data-testid="scan-delete"
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
