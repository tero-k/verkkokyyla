import { useEffect, useState } from "react"
import { mikrotikCreateProfile, mikrotikDeleteProfile, mikrotikListProfiles, mikrotikSetProfilePassword, mikrotikTestConnection, mikrotikUpdateProfile } from "../lib/ipc"
import type { CreateMikrotikProfileRequest, MikrotikProfile, MikrotikTestConnectionDto } from "../lib/types"
import { ConfirmDialog } from "./ConfirmDialog"
import styles from "./MikrotikProfilePanel.module.css"

type Props = { readonly activeProfileId: number | null }
type FormState = CreateMikrotikProfileRequest & { readonly id: number | null; readonly password: string }

const emptyForm: FormState = { id: null, name: "", host: "", port: 443, useTls: true, allowInvalidCerts: false, username: "", password: "" }

function messageFrom(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "object" && error !== null && "message" in error) return String(error.message)
  return String(error)
}

function formFrom(profile: MikrotikProfile): FormState {
  return { id: profile.id, name: profile.name, host: profile.host, port: profile.port, useTls: profile.useTls, allowInvalidCerts: profile.allowInvalidCerts, username: profile.username, password: "" }
}

export function MikrotikProfilePanel({ activeProfileId }: Props) {
  const [profiles, setProfiles] = useState<readonly MikrotikProfile[]>([])
  const [form, setForm] = useState<FormState>(emptyForm)
  const [deleteTarget, setDeleteTarget] = useState<MikrotikProfile | null>(null)
  const [error, setError] = useState("")
  const [testingId, setTestingId] = useState<number | null>(null)
  const [testResult, setTestResult] = useState<MikrotikTestConnectionDto | null>(null)

  async function refresh(): Promise<void> {
    try {
      setProfiles(await mikrotikListProfiles())
    } catch (caught) {
      setError(messageFrom(caught))
    }
  }

  useEffect(() => { void refresh() }, [])

  function setTls(useTls: boolean): void {
    setForm((current) => ({ ...current, useTls, port: current.port === (useTls ? 80 : 443) ? (useTls ? 443 : 80) : current.port }))
  }

  async function save(): Promise<void> {
    setError(""); setTestResult(null)
    const request = { name: form.name, host: form.host, port: form.port, useTls: form.useTls, allowInvalidCerts: form.allowInvalidCerts, username: form.username }
    try {
      const saved = form.id === null ? await mikrotikCreateProfile(request) : await mikrotikUpdateProfile({ ...request, id: form.id })
      if (form.password !== "") await mikrotikSetProfilePassword(saved.id, form.password)
      setForm(emptyForm); await refresh()
    } catch (caught) {
      setError(messageFrom(caught))
    }
  }

  async function remove(profile: MikrotikProfile): Promise<void> {
    try {
      await mikrotikDeleteProfile(profile.id); setDeleteTarget(null); await refresh()
    } catch (caught) {
      setDeleteTarget(null); setError(messageFrom(caught))
    }
  }

  async function testConnection(profile: MikrotikProfile): Promise<void> {
    setTestingId(profile.id); setError(""); setTestResult(null)
    try {
      setTestResult(await mikrotikTestConnection(profile.id))
    } catch (caught) {
      setError(messageFrom(caught))
    } finally {
      setTestingId(null)
    }
  }

  return (
    <section className={styles.panel} aria-label="MikroTik profiles">
      <div className={styles.list}>{profiles.map((profile) => {
        const active = profile.id === activeProfileId
        return <article key={profile.id} data-testid={`profile-row-${profile.id}`} className={styles.profile}><div><h3>{profile.name}</h3><p>{profile.host}:{profile.port} · {profile.useTls ? "HTTPS" : "HTTP"}</p></div><div className={styles.rowActions}><button type="button" onClick={() => setForm(formFrom(profile))} aria-label={`Edit ${profile.name}`}>Edit</button><button type="button" onClick={() => void testConnection(profile)} disabled={testingId === profile.id} aria-label={`Test connection for ${profile.name}`}>Test connection</button><span title={active ? "Cannot delete the profile backing the active session" : undefined}><button type="button" disabled={active} onClick={() => setDeleteTarget(profile)} aria-label={`Delete ${profile.name}`}>Delete</button></span></div></article>
      })}</div>
      <form className={styles.form} onSubmit={(event) => { event.preventDefault(); void save() }}>
        <h2>{form.id === null ? "Create profile" : "Edit profile"}</h2>
        <label>Profile name<input value={form.name} onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, name: value })) }} /></label>
        <label>Host<input value={form.host} onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, host: value })) }} /></label>
        <label>Port<input type="number" min="1" max="65535" value={form.port} onChange={(event) => { const value = event.currentTarget.valueAsNumber; setForm((current) => ({ ...current, port: value })) }} /></label>
        <label className={styles.checkbox}><input type="checkbox" checked={form.useTls} onChange={(event) => setTls(event.currentTarget.checked)} />Use TLS</label>
        {!form.useTls ? <p className={styles.hint}>plain HTTP requires RouterOS v7.9+ and the www service</p> : null}
        <label className={styles.checkbox}><input type="checkbox" checked={form.allowInvalidCerts} onChange={(event) => { const checked = event.currentTarget.checked; setForm((current) => ({ ...current, allowInvalidCerts: checked })) }} />Accept self-signed certificates</label>
        <label>Username<input value={form.username} onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, username: value })) }} /></label>
        <label>Password<input type="password" value={form.password} onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, password: value })) }} /></label>
        <div className={styles.actions}><button type="button" onClick={() => setForm(emptyForm)}>New</button><button type="submit">Save profile</button></div>
      </form>
      {testResult ? <p className={styles.success}>Connection OK: {testResult.boardName ?? "unknown board"} / RouterOS {testResult.routerosVersion ?? "unknown"}</p> : null}
      {error ? <p className={styles.error}>{error}</p> : null}
      {deleteTarget ? <ConfirmDialog message={`Delete profile ${deleteTarget.name}?`} onCancel={() => setDeleteTarget(null)} onConfirm={() => void remove(deleteTarget)} /> : null}
    </section>
  )
}
