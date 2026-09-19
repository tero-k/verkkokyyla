import type { ScanSummaryDto } from "../lib/types"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "./HistoryControls"

import styles from "./ScanSessionPanel.module.css"

type ScanSessionPanelProps = {
  readonly scans: readonly ScanSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
  readonly onDeleteMany?: (ids: readonly number[]) => void | Promise<void>
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
  onDeleteMany,
}: ScanSessionPanelProps) {
  const history = useSessionHistory(scans)
  const canSelect = onDeleteMany != null && scans.length > 0 && !disabled

  const handleDeleteSelected = () => {
    if (onDeleteMany == null) return
    const ids = scans
      .filter((scan) => history.selectedIds.has(scan.id))
      .map((scan) => scan.id)
    if (ids.length === 0) return
    void Promise.resolve(onDeleteMany(ids)).then(() => history.exitSelectMode())
  }

  return (
    <div className={styles.wrapper} data-testid="scan-session-panel">
      <div className={styles.headerRow}>
        <h2>Scan history</h2>
        {canSelect && !history.selectMode && (
          <SelectionToggle onClick={history.enterSelectMode} />
        )}
      </div>
      {history.selectMode && (
        <SelectionBar
          count={history.selectedCount}
          onDelete={handleDeleteSelected}
          onCancel={history.exitSelectMode}
        />
      )}
      {scans.length === 0 ? (
        <p className={styles.empty}>No saved scans yet.</p>
      ) : (
        <ul className={styles.list}>
          {history.visible.map((scan) => (
            <li
              key={scan.id}
              className={`${styles.item}${history.selectedIds.has(scan.id) ? ` ${styles.selected}` : ""}`}
              data-testid="scan-session-item"
            >
              {history.selectMode && (
                <input
                  className={styles.select}
                  type="checkbox"
                  checked={history.selectedIds.has(scan.id)}
                  onChange={() => history.toggleSelected(scan.id)}
                  data-testid="scan-delete-select"
                  aria-label={`Select scan of ${scan.cidr} for deletion`}
                />
              )}
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
      <RevealButton
        totalCount={scans.length}
        hiddenCount={history.hiddenCount}
        expanded={history.expanded}
        onToggle={history.toggleExpanded}
      />
    </div>
  )
}
