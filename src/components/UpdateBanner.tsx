import { openUrl } from "@tauri-apps/plugin-opener"
import type { UpdateInfo } from "../lib/types"
import styles from "./UpdateBanner.module.css"

type Props = {
  update: UpdateInfo
  onDismiss: () => void
}

export default function UpdateBanner({ update, onDismiss }: Props) {
  const openRelease = () => {
    openUrl(update.url).catch(() => {
      // Opening the browser is best-effort; a failed open must not break the
      // shell — the release page stays one web search away.
    })
  }

  return (
    <div className={styles.banner} role="status" data-testid="update-banner">
      <span className={styles.message}>
        Verkkokyylä v{update.version} is available (you have v{update.current}).
      </span>
      <button
        type="button"
        className={styles.viewButton}
        onClick={openRelease}
      >
        View release
      </button>
      <button
        type="button"
        className={styles.dismissButton}
        aria-label="Dismiss update notice"
        onClick={onDismiss}
      >
        ✕
      </button>
    </div>
  )
}
