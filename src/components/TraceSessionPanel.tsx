import type { TraceSummaryDto } from "../lib/types"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "./HistoryControls"
import { Button, Card, SectionHeader } from "./ui/ui"

import styles from "./TraceSessionPanel.module.css"

type TraceSessionPanelProps = {
  readonly sessions: readonly TraceSummaryDto[]
  readonly disabled: boolean
  readonly onOpen: (id: number) => void
  readonly onDelete: (id: number) => void
  readonly onDeleteMany?: (ids: readonly number[]) => void | Promise<void>
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

function statusBadgeClass(status: string): string {
  const normalized = status.toLowerCase()
  if (normalized === "completed") return "vk-badge-success"
  if (normalized === "cancelled") return "vk-badge-warning"
  if (normalized === "error" || normalized === "failed") return "vk-badge-error"
  return ""
}

export function TraceSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
  onDeleteMany,
  compareMode,
  compareSelection,
  onToggleCompare,
  onStartCompareSelection,
  onCompareSelected,
  onCancelCompare,
}: TraceSessionPanelProps) {
  const history = useSessionHistory(sessions)
  const canCompare = sessions.length >= 2 && !disabled && !history.selectMode
  const compareReady = compareSelection.length === 2
  const canSelect = onDeleteMany != null && sessions.length > 0 && !disabled

  const handleDeleteSelected = () => {
    if (onDeleteMany == null) return
    const ids = sessions
      .filter((session) => history.selectedIds.has(session.id))
      .map((session) => session.id)
    if (ids.length === 0) return
    void Promise.resolve(onDeleteMany(ids)).then(() => history.exitSelectMode())
  }

  return (
    <div className={styles.wrapper} data-testid="trace-session-panel">
      <Card className={styles.panel}>
        <div className={styles.panelHeader}>
          <SectionHeader
            title="Trace history"
            aside={`${sessions.length} ${sessions.length === 1 ? "session" : "sessions"}`}
          />
          {canCompare && !compareMode && (
            <Button
              variant="outline-accent"
              small
              onClick={onStartCompareSelection}
              data-testid="trace-compare-toggle"
            >
              Compare traces
            </Button>
          )}
          {canSelect && !compareMode && !history.selectMode && (
            <SelectionToggle onClick={history.enterSelectMode} />
          )}
        </div>
        {compareMode && (
          <div className={styles.compareHeader}>
            <span className={styles.compareHint}>
              Select two traces to compare ({compareSelection.length}/2)
            </span>
            <div className={styles.compareActions}>
              <Button
                variant="primary"
                small
                onClick={onCompareSelected}
                disabled={!compareReady}
                data-testid="trace-compare-button"
              >
                Compare selected
              </Button>
              <Button
                small
                onClick={onCancelCompare}
                data-testid="trace-compare-cancel"
              >
                Cancel
              </Button>
            </div>
          </div>
        )}
        {history.selectMode && (
          <SelectionBar
            count={history.selectedCount}
            onDelete={handleDeleteSelected}
            onCancel={history.exitSelectMode}
          />
        )}
        {sessions.length === 0 ? (
          <p className={styles.empty}>No saved traces yet.</p>
        ) : (
          <ul className={styles.list}>
            {history.visible.map((session) => {
              const selected =
                compareSelection.includes(session.id) ||
                history.selectedIds.has(session.id)
              return (
                <li
                  key={session.id}
                  className={`${styles.item}${selected ? ` ${styles.selected}` : ""}`}
                  data-testid="trace-session-item"
                >
                  {compareMode && (
                    <input
                      className={styles.select}
                      type="checkbox"
                      checked={selected}
                      onChange={() => onToggleCompare(session.id)}
                      disabled={!selected && compareSelection.length >= 2}
                      data-testid="trace-compare-select"
                      aria-label={`Select ${session.targetInput} for comparison`}
                    />
                  )}
                  {history.selectMode && (
                    <input
                      className={styles.select}
                      type="checkbox"
                      checked={history.selectedIds.has(session.id)}
                      onChange={() => history.toggleSelected(session.id)}
                      data-testid="trace-delete-select"
                      aria-label={`Select ${session.targetInput} for deletion`}
                    />
                  )}
                  <div className={styles.summary}>
                    <div className={styles.targetRow}>
                      <span className={styles.target}>{session.targetInput}</span>
                      <span className={`vk-badge ${statusBadgeClass(session.status)}`}>
                        {session.status}
                      </span>
                    </div>
                    <span className={styles.meta}>
                      {session.resolvedIp} · {formatDateTime(session.startedAt)}
                    </span>
                    <span className={styles.metrics}>
                      <span>{session.hopCount} hops</span>
                      <span>{session.family.toUpperCase()}</span>
                      <span className={session.reachedTarget ? styles.reached : styles.partial}>
                        reached {formatReached(session.reachedTarget)}
                      </span>
                    </span>
                  </div>
                  <div className={styles.actions}>
                    <Button
                      small
                      onClick={() => onOpen(session.id)}
                      disabled={disabled}
                      data-testid="trace-open"
                    >
                      Reopen
                    </Button>
                    <Button
                      variant="outline-danger"
                      small
                      onClick={() => onDelete(session.id)}
                      disabled={disabled}
                      data-testid="trace-delete"
                    >
                      Delete
                    </Button>
                  </div>
                </li>
              )
            })}
          </ul>
        )}
        <RevealButton
          totalCount={sessions.length}
          hiddenCount={history.hiddenCount}
          expanded={history.expanded}
          onToggle={history.toggleExpanded}
        />
      </Card>
    </div>
  )
}
