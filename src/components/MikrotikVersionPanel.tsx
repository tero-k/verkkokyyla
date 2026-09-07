import { useState } from "react"
import { mikrotikCheckUpdates, mikrotikFetchChangelog } from "../lib/ipc"
import type { MikrotikFirmwareStatus, MikrotikUpdateStatus } from "../lib/types"
import styles from "./MikrotikVersionPanel.module.css"

type Props = {
  readonly profileId: number | null
  readonly updateStatus: MikrotikUpdateStatus | null
  readonly firmwareStatus: MikrotikFirmwareStatus | null
}

type TypedError = { readonly kind?: string; readonly message: string }

function messageFrom(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "object" && error !== null && "message" in error) {
    const typed: TypedError = { message: String(error.message) }
    return typed.message
  }
  return String(error)
}

function routerosBadge(status: MikrotikUpdateStatus | null): { readonly text: string; readonly className: string } {
  switch (status?.state) {
    case "update-available":
      return { text: "update available", className: styles.warning }
    case "up-to-date":
      return { text: "up to date", className: styles.success }
    case "unknown":
    case undefined:
      return { text: "unknown", className: styles.neutral }
  }
}

function firmwareBadge(status: MikrotikFirmwareStatus | null): { readonly text: string; readonly className: string } {
  switch (status?.state) {
    case "available":
      return { text: "upgrade available", className: styles.warning }
    case "up-to-date":
      return { text: "up to date", className: styles.success }
    case "not-applicable":
      return { text: "Not applicable", className: styles.neutral }
    case "unknown":
    case undefined:
      return { text: "unknown", className: styles.neutral }
  }
}

export function MikrotikVersionPanel({ profileId, updateStatus, firmwareStatus }: Props) {
  const [currentUpdate, setCurrentUpdate] = useState(updateStatus)
  const [currentFirmware, setCurrentFirmware] = useState(firmwareStatus)
  const [busy, setBusy] = useState(false)
  const [changelog, setChangelog] = useState("")
  const [error, setError] = useState("")
  const routeros = currentUpdate ?? updateStatus
  const firmware = currentFirmware ?? firmwareStatus
  const routerBadge = routerosBadge(routeros)
  const boardBadge = firmwareBadge(firmware)
  const changelogVersion = routeros?.latestVersion ?? routeros?.installedVersion ?? null

  async function checkUpdates(): Promise<void> {
    if (profileId === null) return
    setBusy(true); setError("")
    try {
      const result = await mikrotikCheckUpdates(profileId)
      setCurrentUpdate(result.updateStatus); setCurrentFirmware(result.firmwareStatus)
    } catch (caught) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function viewChangelog(): Promise<void> {
    if (changelogVersion === null) return
    setBusy(true); setError(""); setChangelog("")
    try {
      const result = await mikrotikFetchChangelog(changelogVersion)
      setChangelog(result.changelog)
    } catch (caught) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className={styles.panel} aria-label="MikroTik versions">
      <article className={styles.card}>
        <div className={styles.header}><h2>RouterOS</h2><span data-testid="routeros-badge" className={`${styles.badge} ${routerBadge.className}`}>{routerBadge.text}</span></div>
        <dl className={styles.facts}><dt>Installed</dt><dd>{routeros?.installedVersion ?? "unknown"}</dd><dt>Latest</dt><dd data-testid="routeros-latest">{routeros?.latestVersion ?? "unknown"}</dd><dt>Channel</dt><dd>{routeros?.channel ?? "unknown"}</dd></dl>
        <button type="button" onClick={checkUpdates} disabled={busy || profileId === null}>Check for updates</button>
      </article>
      <article className={styles.card}>
        <div className={styles.header}><h2>RouterBOARD firmware</h2><span data-testid="firmware-badge" className={`${styles.badge} ${boardBadge.className}`}>{boardBadge.text}</span></div>
        <dl className={styles.facts}><dt>Current</dt><dd>{firmware?.currentFirmware ?? "unknown"}</dd><dt>Upgrade</dt><dd>{firmware?.upgradeFirmware ?? "unknown"}</dd><dt>Model</dt><dd>{firmware?.model ?? "unknown"}</dd></dl>
        <button type="button" onClick={viewChangelog} disabled={busy || changelogVersion === null}>View changelog</button>
      </article>
      {error ? <p className={styles.mutedError}>{error}</p> : null}
      {changelog ? <pre data-testid="mikrotik-changelog" className={styles.changelog}>{changelog}</pre> : null}
    </section>
  )
}
