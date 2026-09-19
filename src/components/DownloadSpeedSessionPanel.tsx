import type { DownloadSpeedSessionSummaryDto } from "../lib/types"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "./HistoryControls"
import { Button, Card, Meter, SectionHeader } from "./ui/ui"

import styles from "./DownloadSpeedSessionPanel.module.css"

type DownloadSpeedSessionPanelProps = {
  readonly sessions: readonly DownloadSpeedSessionSummaryDto[]
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

function formatMbps(mbps: number): string {
  if (!Number.isFinite(mbps)) return "-"
  return `${mbps.toFixed(2)} Mbps`
}

export function DownloadSpeedSessionPanel({
  sessions,
  disabled,
  onOpen,
  onDelete,
  onDeleteMany,
}: DownloadSpeedSessionPanelProps) {
  const history = useSessionHistory(sessions)
  const canSelect = onDeleteMany != null && sessions.length > 0 && !disabled
  const maximumMbps = Math.max(1, ...sessions.map((session) => session.averageMbps))

  const handleDeleteSelected = () => {
    if (onDeleteMany == null) return
    const ids = sessions
      .filter((session) => history.selectedIds.has(session.id))
      .map((session) => session.id)
    if (ids.length === 0) return
    void Promise.resolve(onDeleteMany(ids)).then(() => history.exitSelectMode())
  }

  return (
    <div data-testid="download-speed-session-panel">
      <Card className={styles.wrapper}>
        <div className={styles.panelHeader}>
          <SectionHeader
            title="Speed test history"
            aside={sessions.length > 0 ? `${sessions.length} saved` : "local sessions"}
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
          <p className={styles.empty}>No saved speed tests yet.</p>
        ) : (
          <ul className={styles.list}>
            {history.visible.map((session) => (
              <li
                key={session.id}
                className={`${styles.item}${history.selectMode ? ` ${styles.selecting}` : ""}${history.selectedIds.has(session.id) ? ` ${styles.selected}` : ""}`}
                data-testid="download-speed-session-item"
              >
                {history.selectMode && (
                  <input
                    className={styles.select}
                    type="checkbox"
                    checked={history.selectedIds.has(session.id)}
                    onChange={() => history.toggleSelected(session.id)}
                    data-testid="download-speed-delete-select"
                    aria-label={`Select ${session.url} for deletion`}
                  />
                )}
                <div className={styles.summary}>
                  <span className={styles.target} title={session.url}>
                    {session.url}
                  </span>
                  <span className={styles.meta}>
                    {session.mode === "page"
                      ? "Full page"
                      : session.mode === "benchmark"
                        ? "Benchmark"
                        : "Single file"}
                    {" · "}
                    {formatDateTime(session.startedAt)}
                  </span>
                  <Meter
                    label={`${session.totalTimeMs} ms`}
                    value={formatMbps(session.averageMbps)}
                    pct={(session.averageMbps / maximumMbps) * 100}
                  />
                </div>
                <div className={styles.actions}>
                  <Button
                    small
                    onClick={() => onOpen(session.id)}
                    disabled={disabled}
                    data-testid="download-speed-open"
                  >
                    Open
                  </Button>
                  <Button
                    small
                    variant="outline-danger"
                    onClick={() => onDelete(session.id)}
                    disabled={disabled}
                    data-testid="download-speed-delete"
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
