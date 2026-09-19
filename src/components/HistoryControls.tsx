import { HISTORY_PAGE_SIZE } from "../lib/constants"
import { Button } from "./ui/ui"

import styles from "./HistoryControls.module.css"

/** Header toggle that enters session multi-select mode. */
export function SelectionToggle({
  onClick,
  disabled = false,
}: {
  readonly onClick: () => void
  readonly disabled?: boolean
}) {
  return (
    <Button
      small
      variant="outline-accent"
      onClick={onClick}
      disabled={disabled}
      data-testid="history-select-toggle"
    >
      Select
    </Button>
  )
}

/** Action strip shown while multi-select mode is active. */
export function SelectionBar({
  count,
  onDelete,
  onCancel,
}: {
  readonly count: number
  readonly onDelete: () => void
  readonly onCancel: () => void
}) {
  return (
    <div className={styles.bar} data-testid="history-selection-bar">
      <span className={styles.hint}>{count} selected</span>
      <div className={styles.actions}>
        <Button
          small
          variant="outline-danger"
          onClick={onDelete}
          disabled={count === 0}
          data-testid="history-delete-selected"
        >
          Delete selected
        </Button>
        <Button
          small
          onClick={onCancel}
          data-testid="history-selection-cancel"
        >
          Cancel
        </Button>
      </div>
    </div>
  )
}

/** "Show N older" / "Show less" footer for capped history lists. */
export function RevealButton({
  totalCount,
  hiddenCount,
  expanded,
  onToggle,
}: {
  readonly totalCount: number
  readonly hiddenCount: number
  readonly expanded: boolean
  readonly onToggle: () => void
}) {
  if (totalCount <= HISTORY_PAGE_SIZE) return null
  return (
    <div className={styles.footer}>
      <Button small onClick={onToggle} data-testid="history-reveal">
        {expanded ? "Show less" : `Show ${hiddenCount} older`}
      </Button>
    </div>
  )
}
