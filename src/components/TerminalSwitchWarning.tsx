import { useState, type CSSProperties } from "react"
import { Button } from "./ui/ui"

import styles from "./TerminalSwitchWarning.module.css"

type TerminalSwitchWarningProps = {
  readonly profileName: string
  readonly profileHost: string
  /** The device's terminal accent — matches the identity tag behind the modal. */
  readonly accent: string
  readonly onDismiss: (silenceRestOfSession: boolean) => void
}

/**
 * Guard against typing into the wrong router: shown when the visible
 * terminal switches to a different device that has an open terminal. The
 * silencer is session-scoped — restarting the app brings the warnings back.
 */
export function TerminalSwitchWarning({
  profileName,
  profileHost,
  accent,
  onDismiss,
}: TerminalSwitchWarningProps) {
  const [silence, setSilence] = useState(false)
  return (
    <div
      className={styles.modalOverlay}
      data-testid="terminal-switch-warning"
      onClick={(event) => {
        if (event.target === event.currentTarget) onDismiss(silence)
      }}
    >
      <div className={styles.modal} role="dialog" aria-modal="true" aria-label="Terminal switched">
        <h2 className={styles.title}>Terminal switched</h2>
        <p className={styles.message}>
          This terminal now drives{" "}
          <strong
            className={styles.device}
            style={{ "--terminal-accent": accent } as CSSProperties}
          >
            {profileName}
          </strong>{" "}
          <span className={styles.host}>({profileHost})</span>. Commands you type run on that
          device.
        </p>
        <label className={styles.silenceRow}>
          <input
            type="checkbox"
            checked={silence}
            onChange={(event) => setSilence(event.currentTarget.checked)}
          />
          Don&apos;t warn again this session
        </label>
        <div className={styles.actions}>
          <Button
            variant="primary"
            data-testid="terminal-switch-warning-dismiss"
            onClick={() => onDismiss(silence)}
          >
            Got it
          </Button>
        </div>
      </div>
    </div>
  )
}
