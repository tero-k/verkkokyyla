import type { TraceSummaryDto } from "../lib/types"

import styles from "./TraceSessionPanel.module.css"

type TraceSessionPanelProps = {
  readonly sessions: readonly TraceSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
  readonly compareMode: boolean
  readonly compareSelection: readonly number[]
  readonly onToggleCompare: (id: number) => void
  readonly onStartCompareSelection: () => void
  readonly onCompareSelected: () => void
  readonly onCancelCompare: () => void
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
  compareMode,
  compareSelection,
  onToggleCompare,
  onStartCompareSelection,
  onCompareSelected,
  onCancelCompare,
}: TraceSessionPanelProps) {
  const canCompare = sessions.length >= 2 && !disabled
  const compareReady = compareSelection.length === 2

  return (
    <div className={styles.wrapper} data-testid="trace-session-panel">
      <h2>Trace history</h2>
      {canCompare && !compareMode && (
        <div className={styles.compareHeader}>
          <button
            type="button"
            onClick={onStartCompareSelection}
            data-testid="trace-compare-toggle"
          >
            Compare traces
          </button>
        </div>
      )}
      {compareMode && (
        <div className={styles.compareHeader}>
          <span className={styles.compareHint}>
            Select two traces to compare ({compareSelection.length}/2)
          </span>
          <button
            type="button"
            onClick={onCompareSelected}
            disabled={!compareReady}
            data-testid="trace-compare-button"
          >
            Compare selected
          </button>
          <button
            type="button"
            onClick={onCancelCompare}
            data-testid="trace-compare-cancel"
          >
            Cancel
          </button>
        </div>
      )}
      {sessions.length === 0 ? (
        <p className={styles.empty}>No saved traces yet.</p>
      ) : (
        <ul className={styles.list}>
          {sessions.map((session) => (
            <li key={session.id} className={styles.item} data-testid="trace-session-item">
              <div className={styles.summary}>
                {compareMode && (
                  <input
                    type="checkbox"
                    checked={compareSelection.includes(session.id)}
                    onChange={() => onToggleCompare(session.id)}
                    disabled={!compareSelection.includes(session.id) && compareSelection.length >= 2}
                    data-testid="trace-compare-select"
                    aria-label={`Select ${session.targetInput} for comparison`}
                  />
                )}
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
