import styles from "./ConfirmDialog.module.css"

type ConfirmDialogProps = {
  readonly message: string
  readonly confirmLabel?: string
  readonly onConfirm: () => void
  readonly onCancel: () => void
}

/**
 * In-app confirmation modal. Replaces `window.confirm`, which shows the
 * WebView origin ("tauri.localhost says") inside the Tauri shell.
 */
export function ConfirmDialog({
  message,
  confirmLabel = "Delete",
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <div
      className={styles.modalOverlay}
      data-testid="confirm-dialog"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onCancel()
        }
      }}
    >
      <div className={styles.modal} role="dialog" aria-modal="true">
        <p className={styles.message}>{message}</p>
        <div className={styles.actions}>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className={styles.confirm}
            data-testid="confirm-dialog-confirm"
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
