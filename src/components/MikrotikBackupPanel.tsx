import { open } from "@tauri-apps/plugin-dialog"
import { useEffect, useMemo, useState } from "react"
import { mikrotikBackup, mikrotikGetBackupDestination, mikrotikSetBackupDestination } from "../lib/ipc"
import { ConfirmDialog } from "./ConfirmDialog"
import styles from "./MikrotikBackupPanel.module.css"

type Props = {
  readonly profileId: number | null
  readonly onCreated?: () => void
}
type BackupResult = { readonly backupPath: string; readonly exportPath: string | null; readonly cleanupWarnings: readonly string[] }
type TypedError = { readonly kind?: string; readonly message: string }

const BACKUP_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i

function defaultBackupName(now = new Date()): string {
  const two = (value: number) => String(value).padStart(2, "0")
  return `verkkokyyla-${now.getFullYear()}${two(now.getMonth() + 1)}${two(now.getDate())}-${two(now.getHours())}${two(now.getMinutes())}${two(now.getSeconds())}`
}

function messageFrom(error: unknown): TypedError {
  if (error instanceof Error) return { message: error.message }
  if (typeof error === "object" && error !== null && "message" in error) {
    return {
      kind: "kind" in error ? String(error.kind) : undefined,
      message: String(error.message),
    }
  }
  return { message: String(error) }
}

function validName(name: string): boolean {
  return BACKUP_NAME.test(name) && !RESERVED.test(name)
}

export function MikrotikBackupPanel({ profileId, onCreated }: Props) {
  const [name, setName] = useState(defaultBackupName)
  const [password, setPassword] = useState("")
  const [includeRsc, setIncludeRsc] = useState(true)
  const [destination, setDestination] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [result, setResult] = useState<BackupResult | null>(null)
  const [confirmOverwrite, setConfirmOverwrite] = useState(false)
  const nameIsValid = validName(name)
  const canSubmit = profileId !== null && destination !== "" && nameIsValid && !busy

  const savedPaths = useMemo(() => {
    if (result === null) return []
    return result.exportPath === null ? [result.backupPath] : [result.backupPath, result.exportPath]
  }, [result])

  // Prefill the remembered destination directory (app-wide setting).
  useEffect(() => {
    let mounted = true
    void mikrotikGetBackupDestination()
      .then((saved) => {
        if (mounted && saved !== null && saved !== "") setDestination(saved)
      })
      .catch(() => {
        // No remembered destination yet (or unreadable): start empty.
      })
    return () => {
      mounted = false
    }
  }, [])

  async function chooseDirectory(): Promise<void> {
    const selected = await open({ directory: true, defaultPath: destination || undefined })
    if (typeof selected === "string") {
      setDestination(selected)
      // Remember the pick for next time; failure only loses the memory.
      try {
        await mikrotikSetBackupDestination(selected)
      } catch {
        // non-fatal: the backup can still proceed with the chosen path
      }
    }
  }

  async function run(overwrite: boolean): Promise<void> {
    if (profileId === null || !nameIsValid || destination === "") return
    setBusy(true); setError(""); setResult(null)
    try {
      const next = await mikrotikBackup(profileId, destination, name, password || undefined, includeRsc, overwrite)
      setResult(next); setPassword(""); onCreated?.()
      // A successful backup confirms the destination works: remember it.
      try {
        await mikrotikSetBackupDestination(destination)
      } catch {
        // non-fatal
      }
    } catch (caught) {
      const typed = messageFrom(caught)
      if (typed.kind === "OutputExists" && !overwrite) {
        setConfirmOverwrite(true)
      } else if (typed.kind === "SshUnreachable") {
        setError(`${typed.message} Enable the SSH service on the router (IP > Services)`)
      } else {
        setError(typed.message)
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className={styles.panel} aria-label="MikroTik backups" data-testid="mikrotik-backup-panel">
      <header className={styles.header}>
        <h2>Create backup</h2>
        <p>Save a RouterOS backup from the selected profile to a local directory.</p>
      </header>
      <form className={styles.form} onSubmit={(event) => { event.preventDefault(); void run(false) }}>
        <div className={styles.fields}>
          <label className={styles.field}>Backup name<input value={name} aria-invalid={!nameIsValid} onChange={(event) => setName(event.currentTarget.value)} /></label>
          <label className={styles.field}>Encryption password (optional)<input type="password" value={password} onChange={(event) => setPassword(event.currentTarget.value)} /></label>
        </div>
        {!nameIsValid ? <p className={`${styles.message} ${styles.error}`}>Use 1-64 letters, numbers, dot, underscore, or dash; no Windows device names.</p> : null}
        <label className={`${styles.field} ${styles.checkbox}`}><input type="checkbox" checked={includeRsc} onChange={(event) => setIncludeRsc(event.currentTarget.checked)} />Include .rsc export</label>
        <div className={styles.directory}>
          <span className={styles.directoryLabel}>Destination directory</span>
          <div className={styles.directoryRow}>
            <button type="button" disabled={busy} onClick={chooseDirectory}>Choose directory</button>
            <p className={`${styles.message} ${styles.destination}`}>{destination || "No directory selected"}</p>
          </div>
        </div>
        {profileId === null ? <p className={styles.message}>Select a profile above to create a backup.</p> : null}
        {error ? <p className={`${styles.message} ${styles.error}`} role="alert">{error}</p> : null}
        {busy ? <p className={`${styles.message} ${styles.progress}`} role="status">Creating backup...</p> : null}
        <div className={styles.actions}><button className={styles.submit} type="submit" disabled={!canSubmit}>{busy ? "Creating..." : "Create backup"}</button></div>
      </form>
      {confirmOverwrite ? <ConfirmDialog message="Backup output already exists. Overwrite it?" confirmLabel="Overwrite" onCancel={() => setConfirmOverwrite(false)} onConfirm={() => { setConfirmOverwrite(false); void run(true) }} /> : null}
      {savedPaths.length > 0 ? <section className={styles.success} aria-label="Saved backup files"><h3>Saved files</h3><ul className={styles.paths}>{savedPaths.map((path) => <li key={path}>{path}</li>)}</ul></section> : null}
    </section>
  )
}
