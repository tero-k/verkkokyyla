import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react"
import { Terminal } from "@xterm/xterm"
import { FitAddon } from "@xterm/addon-fit"
import { Button, Live } from "./ui/ui"

import styles from "./MikrotikTerminalPanel.module.css"
import "@xterm/xterm/css/xterm.css"

/** Injected SSH transport: the panel owns the xterm lifecycle, the adapter
    owns the IPC (MikroTik numeric terminal ids, OPNsense string ids,
    OPNsense's extra credential/fingerprint payload). */
export type SshTerminalAdapter = {
  readonly open: (
    cols: number,
    rows: number,
    onData: (chunk: string) => void,
    extra?: unknown,
  ) => Promise<string | number>
  readonly write: (terminalId: string | number, dataBase64: string) => Promise<void>
  readonly resize: (terminalId: string | number, cols: number, rows: number) => Promise<void>
  readonly close: (terminalId: string | number) => Promise<void>
}

/** A rejected `open`, kept typed so host-key prompts can react to the
    `unknown-host-key` / `host-key-mismatch` kinds specifically. */
export type SshOpenError = {
  readonly kind: string
  readonly message: string
  readonly fingerprint?: string
}

export type SshTerminalPanelProps = {
  readonly profileId: number
  readonly profileName: string
  readonly profileHost: string
  readonly adapter: SshTerminalAdapter
  /** Prefix for the panel's data-testids (`${prefix}-panel`, `-identity`,
      `-container`, `-empty`). */
  readonly testIdPrefix?: string
  /** Value passed as `extra` to `adapter.open` on every connect attempt —
      e.g. OPNsense's confirmed host-key fingerprint. The latest value is
      read at connect time, so a prompt can update it and retry. */
  readonly connectExtra?: unknown
  /** Idle/connect-failed slot rendered between the toolbar and the terminal
      (credential fields, host-key verification prompts). `connect` retries
      the open; pass `extra` to override `connectExtra` for that one attempt
      (e.g. the confirmed host-key fingerprint). */
  readonly renderIdle?: (ctx: {
    readonly openError: SshOpenError | null
    readonly connecting: boolean
    readonly connect: (extra?: unknown) => void
  }) => ReactNode
  /** Terminal session opened — the view keeps this panel alive across device switches. */
  readonly onActivated: (profileId: number) => void
  /** Deliberate disconnect — the view drops the device from the persistent set. */
  readonly onDeactivated: (profileId: number) => void
}

/** Stable per-device accent color: the terminal's identity tag and the
    device-strip pill share it, pairing a chip with its shell at a glance —
    the guard against configuring the wrong device. */
export function terminalAccentFor(profileId: number): string {
  return `hsl(${(profileId * 47) % 360} 75% 55%)`
}

type TerminalStatus = "idle" | "connecting" | "connected"

/** SSH bytes travel the IPC boundary as base64 chunks — the channel layer
    is JSON, and shell output is arbitrary bytes, not UTF-8 text. */
function encodeBase64(bytes: Uint8Array): string {
  let binary = ""
  const step = 0x8000
  for (let index = 0; index < bytes.length; index += step) {
    binary += String.fromCharCode(...bytes.subarray(index, index + step))
  }
  return btoa(binary)
}

function decodeBase64(data: string): Uint8Array {
  const binary = atob(data)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

/** Tauri command errors deserialize as `{ kind, message }`, not Error
    instances — surface the message the backend typed. */
function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "object" && err !== null && "message" in err) {
    const message: unknown = err.message
    return String(message)
  }
  return String(err)
}

function openErrorOf(err: unknown): SshOpenError {
  if (typeof err === "object" && err !== null && "message" in err) {
    const kind: unknown = "kind" in err ? err.kind : undefined
    const fingerprint: unknown = "fingerprint" in err ? err.fingerprint : undefined
    const message: unknown = err.message
    return {
      kind: typeof kind === "string" ? kind : "error",
      message: String(message),
      ...(typeof fingerprint === "string" ? { fingerprint } : {}),
    }
  }
  return { kind: "error", message: errorMessage(err) }
}

export function SshTerminalPanel({
  profileId,
  profileName,
  profileHost,
  adapter,
  testIdPrefix = "ssh-terminal",
  connectExtra,
  renderIdle,
  onActivated,
  onDeactivated,
}: SshTerminalPanelProps) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const terminalIdRef = useRef<string | number | null>(null)
  const observerRef = useRef<ResizeObserver | null>(null)
  const generationRef = useRef(0)
  const connectExtraRef = useRef<unknown>(connectExtra)
  const [status, setStatus] = useState<TerminalStatus>("idle")
  const [error, setError] = useState<string | null>(null)
  const [openError, setOpenError] = useState<SshOpenError | null>(null)

  useEffect(() => {
    connectExtraRef.current = connectExtra
  }, [connectExtra])

  /** Tear everything down and close the backend terminal (if it received
      an id). Safe to call while still halfway through `connect`. */
  async function teardown(): Promise<void> {
    generationRef.current += 1
    observerRef.current?.disconnect()
    observerRef.current = null
    termRef.current?.dispose()
    termRef.current = null
    fitRef.current = null
    const terminalId = terminalIdRef.current
    terminalIdRef.current = null
    if (terminalId !== null) {
      await adapter.close(terminalId).catch(() => undefined)
    }
  }

  // One panel instance per profile (keyed in the view): a device switch
  // hides this panel instead of tearing it down, so profileId never changes
  // for a given instance. Only unmount — deactivation or profile deletion —
  // closes the backend session, via the effect below.

  // Unmount: never leave a backend terminal running without a view. No
  // setState here — the component is gone.
  useEffect(() => {
    return () => {
      generationRef.current += 1
      observerRef.current?.disconnect()
      termRef.current?.dispose()
      const terminalId = terminalIdRef.current
      terminalIdRef.current = null
      if (terminalId !== null) {
        void adapter.close(terminalId).catch(() => undefined)
      }
    }
  }, [adapter])

  async function connect(openExtra?: unknown): Promise<void> {
    if (containerRef.current === null || status !== "idle") return
    setStatus("connecting")
    setError(null)
    setOpenError(null)
    const generation = generationRef.current
    const container = containerRef.current
    const extra = openExtra !== undefined ? openExtra : connectExtraRef.current
    const term = new Terminal({ convertEol: true, cursorBlink: true, fontSize: 13 })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(container)
    fit.fit()
    /** The backend session died mid-life (a write/resize was rejected):
        tear the view down and return to idle so the user can reconnect.
        Idempotent — the first call nulls the id, later failures no-op. */
    const handleSessionDead = (): void => {
      if (terminalIdRef.current === null) return
      terminalIdRef.current = null
      observerRef.current?.disconnect()
      observerRef.current = null
      termRef.current?.dispose()
      termRef.current = null
      fitRef.current = null
      setStatus("idle")
      setError("Terminal session ended unexpectedly")
    }

    const dataSubscription = term.onData((data) => {
      const terminalId = terminalIdRef.current
      if (terminalId !== null) {
        void adapter
          .write(terminalId, encodeBase64(new TextEncoder().encode(data)))
          .catch(() => handleSessionDead())
      }
    })
    try {
      const opened = await adapter.open(term.cols, term.rows, (chunk) => {
        term.write(decodeBase64(chunk))
      }, extra)
      if (generationRef.current !== generation) {
        // Disconnected or profile-changed while the open was in flight —
        // don't orphan the backend terminal.
        dataSubscription.dispose()
        term.dispose()
        void adapter.close(opened).catch(() => undefined)
        return
      }
      terminalIdRef.current = opened
      termRef.current = term
      fitRef.current = fit
      if (typeof ResizeObserver !== "undefined") {
        const observer = new ResizeObserver((entries) => {
          const rect = entries[0]?.contentRect
          // Hidden while another device's terminal is selected: zero
          // geometry. Keep the last size — refit happens on resurfacing.
          if (rect !== undefined && (rect.width < 1 || rect.height < 1)) return
          const currentFit = fitRef.current
          const currentTerm = termRef.current
          const terminalId = terminalIdRef.current
          if (currentFit === null || currentTerm === null || terminalId === null) return
          currentFit.fit()
          void adapter.resize(terminalId, currentTerm.cols, currentTerm.rows).catch(() =>
            handleSessionDead(),
          )
        })
        observer.observe(container)
        observerRef.current = observer
      }
      setStatus("connected")
      onActivated(profileId)
    } catch (err) {
      dataSubscription.dispose()
      term.dispose()
      if (generationRef.current === generation) {
        // Stay retryable: a failed open returns to idle with the banner
        // (and the typed error for credential/host-key prompts) instead of
        // wedging the panel until a remount.
        setStatus("idle")
        setOpenError(openErrorOf(err))
        setError(errorMessage(err))
      }
    }
  }

  async function disconnect(): Promise<void> {
    await teardown()
    setStatus("idle")
    setOpenError(null)
    onDeactivated(profileId)
  }

  const idleCtx = {
    openError,
    connecting: status === "connecting",
    connect: (extra?: unknown) => void connect(extra),
  }

  return (
    <div className={styles.panel} data-testid={`${testIdPrefix}-panel`}>
      <section className={styles.section} aria-label="SSH terminal">
        <h2>Terminal</h2>

        <header
          className={styles.identity}
          data-testid={`${testIdPrefix}-identity`}
          style={{ "--terminal-accent": terminalAccentFor(profileId) } as CSSProperties}
        >
          <span className={styles.identityKicker}>Active device</span>
          <span className={styles.identityName}>{profileName}</span>
          <span className={styles.identityHost}>{profileHost}</span>
        </header>

        <div className={styles.toolbar}>
          <div className={styles.controls}>
            <Button
              variant="primary"
              onClick={() => void connect()}
              disabled={status !== "idle" || profileId === null}
            >
              Connect
            </Button>
            <Button
              variant="outline-accent"
              onClick={() => void disconnect()}
              disabled={status !== "connected"}
            >
              Disconnect
            </Button>
            {status === "connected" ? <Live /> : null}
          </div>
        </div>

        {status === "idle" && renderIdle !== undefined ? renderIdle(idleCtx) : null}

        {error ? (
          <p className={styles.banner} role="alert">
            {error}
          </p>
        ) : null}

        <div
          className={styles.terminal}
          ref={containerRef}
          data-testid={`${testIdPrefix}-container`}
        />

        {status === "idle" ? (
          <p className={styles.empty} data-testid={`${testIdPrefix}-empty`}>
            No terminal session. Connect opens a shell on {profileName} ({profileHost}).
          </p>
        ) : null}
        {status === "connecting" ? <p className={styles.empty}>Connecting…</p> : null}
      </section>
    </div>
  )
}
