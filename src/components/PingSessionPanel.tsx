import type { SessionSummaryDto } from "../lib/types"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "./HistoryControls"
import { Button, Card, SectionHeader } from "./ui/ui"

import styles from "./PingSessionPanel.module.css"

type PingSessionPanelProps = {
  readonly sessions: readonly SessionSummaryDto[]
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

export function PingSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
  onDeleteMany,
}: PingSessionPanelProps) {
  const history = useSessionHistory(sessions)
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
    <section
      className={`${styles.wrapper}${disabled ? ` ${styles.disabled}` : ""}`}
      data-testid="session-panel"
      aria-label="Past ping sessions"
    >
      <Card className={styles.panel}>
        <div className={styles.header}>
          <SectionHeader
            title="Past sessions"
            aside={`${sessions.length} saved`}
          />
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

        {sessions.length === 0 ? (
          <p className={styles.empty}>No saved sessions yet.</p>
        ) : (
          <>
            <div
              className={`vk-table-head ${styles.tableHead}${history.selectMode ? ` ${styles.selecting}` : ""}`}
              aria-hidden="true"
            >
              {history.selectMode && <span />}
              <span>Target</span>
              <span>Started</span>
              <span>Packets</span>
              <span>Loss</span>
              <span>Actions</span>
            </div>
            <ul className={styles.list}>
              {history.visible.map((session) => (
                <li
                  key={session.id}
                  className={`vk-table-row ${styles.item}${history.selectMode ? ` ${styles.selecting}` : ""}${history.selectedIds.has(session.id) ? ` ${styles.selected}` : ""}`}
                  data-testid="session-item"
                >
                  {history.selectMode && (
                    <input
                      className={styles.select}
                      type="checkbox"
                      checked={history.selectedIds.has(session.id)}
                      onChange={() => history.toggleSelected(session.id)}
                      data-testid="session-delete-select"
                      aria-label={`Select ${session.targetInput} for deletion`}
                    />
                  )}
                  <div className={styles.summary}>
                    <span className={styles.target} title={session.targetInput}>
                      {session.targetInput}
                    </span>
                    <span
                      className={styles.endpoint}
                      title={`${session.resolvedIp} · ${session.family}`}
                    >
                      {session.resolvedIp} · {session.family}
                    </span>
                  </div>
                  <time
                    className={styles.timestamp}
                    dateTime={session.startedAt}
                  >
                    {formatDateTime(session.startedAt)}
                  </time>
                  <span className={styles.probes}>
                    {session.probeCount} probes
                  </span>
                  <span
                    className={`${styles.loss}${session.lossPercent > 0 ? ` ${styles.hasLoss}` : ""}`}
                  >
                    {session.lossPercent.toFixed(2)}% loss
                  </span>
                  <div className={styles.actions}>
                    <Button
                      small
                      onClick={() => onOpen(session.id)}
                      disabled={disabled}
                      data-testid="session-open"
                    >
                      Reopen
                    </Button>
                    <Button
                      small
                      variant="outline-danger"
                      onClick={() => onDelete(session.id)}
                      disabled={disabled}
                      data-testid="session-delete"
                    >
                      Delete
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
        <RevealButton
          totalCount={sessions.length}
          hiddenCount={history.hiddenCount}
          expanded={history.expanded}
          onToggle={history.toggleExpanded}
        />
      </Card>
    </section>
  )
}
