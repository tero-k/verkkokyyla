import { open } from "@tauri-apps/plugin-dialog"
import { useMemo, useState } from "react"
import { mikrotikBackup } from "../lib/ipc"
import { ConfirmDialog } from "./ConfirmDialog"
import styles from "./MikrotikBackupButton.module.css"

type Props = { readonly profileId: number | null }
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

export function MikrotikBackupButton({ profileId }: Props) {
  const [openDialog, setOpenDialog] = useState(false)
  const [name, setName] = useState(defaultBackupName)
  const [password, setPassword] = useState("")
  const [includeRsc, setIncludeRsc] = useState(false)
  const [destination, setDestination] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [result, setResult] = useState<BackupResult | null>(null)
  const [confirmOverwrite, setConfirmOverwrite] = useState(false)
  const canSubmit = profileId !== null && destination !== "" && validName(name) && !busy

  const savedPaths = useMemo(() => {
    if (result === null) return []
    return result.exportPath === null ? [result.backupPath] : [result.backupPath, result.exportPath]
  }, [result])

  function resetDialog(): void {
    setOpenDialog(false); setName(defaultBackupName()); setPassword(""); setIncludeRsc(false)
    setDestination(""); setBusy(false); setConfirmOverwrite(false)
  }

  async function chooseDirectory(): Promise<void> {
    const selected = await open({ directory: true })
    if (typeof selected === "string") setDestination(selected)
  }

  async function run(overwrite: boolean): Promise<void> {
    if (profileId === null || !validName(name) || destination === "") return
    setBusy(true); setError(""); setResult(null)
    try {
      const next = await mikrotikBackup(profileId, destination, name, password || undefined, includeRsc, overwrite)
      setResult(next); setPassword(""); setOpenDialog(false)
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
    <div className={styles.wrapper}>
      <button type="button" data-testid="mikrotik-backup-button" disabled={profileId === null} onClick={() => setOpenDialog(true)}>Backup</button>
      {openDialog ? (
        <div className={styles.overlay} role="dialog" aria-modal="true" aria-label="Create MikroTik backup">
          <section className={styles.dialog}>
            <h2>Create backup</h2>
            <label>Backup name<input value={name} onChange={(event) => setName(event.currentTarget.value)} /></label>
            {!validName(name) ? <p className={styles.error}>Use 1-64 letters, numbers, dot, underscore, or dash; no Windows device names.</p> : null}
            <label>Encryption password (optional)<input type="password" value={password} onChange={(event) => setPassword(event.currentTarget.value)} /></label>
            <label className={styles.checkbox}><input type="checkbox" checked={includeRsc} onChange={(event) => setIncludeRsc(event.currentTarget.checked)} />Include .rsc export</label>
            <button type="button" onClick={chooseDirectory}>Choose directory</button>
            {destination ? <p className={styles.destination}>{destination}</p> : null}
            {error ? <p className={styles.error}>{error}</p> : null}
            <div className={styles.actions}><button type="button" onClick={resetDialog}>Cancel</button><button type="button" disabled={!canSubmit} onClick={() => void run(false)}>{busy ? "Creating..." : "Create backup"}</button></div>
          </section>
        </div>
      ) : null}
      {confirmOverwrite ? <ConfirmDialog message="Backup output already exists. Overwrite it?" confirmLabel="Overwrite" onCancel={() => setConfirmOverwrite(false)} onConfirm={() => { setConfirmOverwrite(false); void run(true) }} /> : null}
      {savedPaths.length > 0 ? <ul className={styles.paths}>{savedPaths.map((path) => <li key={path}>{path}</li>)}</ul> : null}
    </div>
  )
}
