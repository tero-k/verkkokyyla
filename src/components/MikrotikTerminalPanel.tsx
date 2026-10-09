import { useMemo } from "react"
import {
  mikrotikTerminalClose,
  mikrotikTerminalOpen,
  mikrotikTerminalResize,
  mikrotikTerminalWrite,
} from "../lib/ipc"
import { SshTerminalPanel, terminalAccentFor, type SshTerminalAdapter } from "./SshTerminalPanel"

export { terminalAccentFor }

type MikrotikTerminalPanelProps = {
  readonly profileId: number
  readonly profileName: string
  readonly profileHost: string
  /** Terminal session opened — the view keeps this panel alive across device switches. */
  readonly onActivated: (profileId: number) => void
  /** Deliberate disconnect — the view drops the device from the persistent set. */
  readonly onDeactivated: (profileId: number) => void
}

/** MikroTik adapter over the shared {@link SshTerminalPanel}: the xterm /
    base64 / resize / close lifecycle lives in the shared panel, this file
    only maps it onto the numeric-id MikroTik terminal IPC. */
export function MikrotikTerminalPanel({ profileId, ...rest }: MikrotikTerminalPanelProps) {
  const adapter = useMemo<SshTerminalAdapter>(
    () => ({
      open: (cols, rows, onData) =>
        mikrotikTerminalOpen(profileId, cols, rows, onData).then((opened) => opened.terminalId),
      write: (terminalId, data) => mikrotikTerminalWrite(Number(terminalId), data),
      resize: (terminalId, cols, rows) => mikrotikTerminalResize(Number(terminalId), cols, rows),
      close: (terminalId) => mikrotikTerminalClose(Number(terminalId)),
    }),
    [profileId],
  )
  return (
    <SshTerminalPanel
      {...rest}
      profileId={profileId}
      adapter={adapter}
      testIdPrefix="mikrotik-terminal"
    />
  )
}
