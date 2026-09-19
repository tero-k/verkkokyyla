import type { MikrotikSessionSummaryDto } from "../lib/types"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "./HistoryControls"
import { Button, Card, SectionHeader } from "./ui/ui"

import styles from "./TraceSessionPanel.module.css"

type MikrotikSessionPanelProps = {
  readonly sessions: readonly MikrotikSessionSummaryDto[]
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
  onDeleteMany,
}: MikrotikSessionPanelProps) {
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
    <div className={styles.wrapper} data-testid="mikrotik-session-panel">
      <Card className={styles.panel}>
        <div className={styles.panelHeader}>
          <SectionHeader
            title="MikroTik history"
            aside={`${sessions.length} ${sessions.length === 1 ? "session" : "sessions"}`}
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
          <p className={styles.empty}>No saved MikroTik sessions yet.</p>
        ) : (
          <ul className={styles.list}>
            {history.visible.map((session) => (
              <li
                key={session.id}
                className={`${styles.item}${history.selectedIds.has(session.id) ? ` ${styles.selected}` : ""}`}
                data-testid="mikrotik-session-item"
              >
                {history.selectMode && (
                  <input
                    className={styles.select}
                    type="checkbox"
                    checked={history.selectedIds.has(session.id)}
                    onChange={() => history.toggleSelected(session.id)}
                    data-testid="mikrotik-delete-select"
                    aria-label={`Select session ${session.id} for deletion`}
                  />
                )}
                <div className={styles.summary}>
                  <span className={styles.target} title={formatDevice(session)}>{formatDevice(session)}</span>
                  <span className={styles.meta} title={`${formatDateTime(session.startedAt)} - ${formatEnded(session.endedAt)}`}>
                    {formatDateTime(session.startedAt)} - {formatEnded(session.endedAt)}
                  </span>
                  <span className={styles.meta} title={`${session.snapshotCount} snapshots - ${session.status}`}>
                    {session.snapshotCount} snapshots - {session.status}
                  </span>
                </div>
                <div className={styles.actions}>
                  <Button
                    small
                    onClick={() => onOpen(session.id)}
                    disabled={disabled}
                    data-testid="mikrotik-open-session"
                  >
                    Load
                  </Button>
                  <Button
                    small
                    variant="outline-danger"
                    onClick={() => onDelete(session.id)}
                    disabled={disabled}
                    data-testid="mikrotik-delete-session"
                  >
                    Delete
                  </Button>
                </div>
              </li>
            ))}
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
