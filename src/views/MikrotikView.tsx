import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react"
import { MikrotikBackupPanel } from "../components/MikrotikBackupPanel"
import { MikrotikBackupLibrary } from "../components/MikrotikBackupLibrary"
import { MikrotikGraphs } from "../components/MikrotikGraphs"
import { MikrotikInterfaceTable } from "../components/MikrotikInterfaceTable"
import { MikrotikLogsPanel } from "../components/MikrotikLogsPanel"
import { MikrotikProfilePanel } from "../components/MikrotikProfilePanel"
import { MikrotikSessionPanel } from "../components/MikrotikSessionPanel"
import { MikrotikStatusCards } from "../components/MikrotikStatusCards"
import { MikrotikTerminalPanel, terminalAccentFor } from "../components/MikrotikTerminalPanel"
import { TerminalSwitchWarning } from "../components/TerminalSwitchWarning"
import { MikrotikVersionPanel } from "../components/MikrotikVersionPanel"
import { MikrotikVlanPanel } from "../components/MikrotikVlanPanel"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { useMikrotik } from "../hooks/useMikrotik"
import type { MikrotikProfile } from "../lib/types"
import { Button, Live, ViewHeader } from "../components/ui/ui"

import styles from "./MikrotikView.module.css"

const TABS = [
  { id: "profiles", label: "Profiles" },
  { id: "backups", label: "Backups" },
  { id: "system", label: "System" },
  { id: "interfaces", label: "Interfaces" },
  { id: "vlans", label: "VLANs" },
  { id: "logs", label: "Logs" },
  { id: "terminal", label: "Terminal" },
] as const
const SYSTEM_CHARTS = ["cpu", "memory"] as const
const INTERFACE_CHARTS = ["interface"] as const

type TabId = (typeof TABS)[number]["id"]

export default function MikrotikView() {
  const mikrotik = useMikrotik()
  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const [activeTab, setActiveTab] = useState<TabId>("profiles")
  const [backupsRefreshKey, setBackupsRefreshKey] = useState(0)
  const [selectedInterfaceName, setSelectedInterfaceName] = useState<string | null>(null)
  /** Profiles with an activated terminal. Each gets a persistent panel
      instance (keyed by profile id) that stays mounted — hidden, not
      unmounted — while another device is selected, so switching devices
      never drops a shell. */
  const [terminalProfiles, setTerminalProfiles] = useState<readonly number[]>([])
  /** Session-scoped silencer for the terminal-switch warning — never
      persisted, so a restart brings the warnings back. */
  const terminalWarningSilencedRef = useRef(false)
  const [terminalSwitchWarning, setTerminalSwitchWarning] = useState<MikrotikProfile | null>(null)
  const interfaces = mikrotik.latestSnapshot?.interfaces ?? []
  const selectedInterface = useMemo(() => {
    if (interfaces.some((item) => item.name === selectedInterfaceName)) {
      return selectedInterfaceName
    }
    return interfaces[0]?.name ?? null
  }, [interfaces, selectedInterfaceName])
  const metadata = mikrotik.loadedSession?.session ?? null
  const detachedProfileId =
    mikrotik.selectedDevice !== null && mikrotik.selectedDevice.running && !mikrotik.selectedDevice.attached
      ? mikrotik.selectedDevice.profileId
      : null

  useEffect(() => {
    setSelectedInterfaceName((current) =>
      interfaces.some((item) => item.name === current) ? current : (interfaces[0]?.name ?? null),
    )
  }, [interfaces])

  // Prune terminals for deleted profiles — unmounting the panel closes the
  // backend session via the panel's unmount effect.
  useEffect(() => {
    setTerminalProfiles((current) =>
      current.filter((id) => mikrotik.profiles.some((profile) => profile.id === id)),
    )
  }, [mikrotik.profiles])

  const activateTerminal = useCallback((profileId: number) => {
    setTerminalProfiles((current) =>
      current.includes(profileId) ? current : [...current, profileId],
    )
  }, [])
  const deactivateTerminal = useCallback((profileId: number) => {
    setTerminalProfiles((current) => current.filter((id) => id !== profileId))
  }, [])

  // Warn when the visible terminal switches to a different device that has
  // an open terminal — the moment a wrong-router mistake becomes possible.
  // Switching to a device without a terminal (or while on another tab) shows
  // no live shell, so no warning is needed there.
  const previousTerminalProfileRef = useRef<number | null>(null)
  useEffect(() => {
    const current = mikrotik.selectedProfile?.id ?? null
    const previous = previousTerminalProfileRef.current
    previousTerminalProfileRef.current = current
    if (
      activeTab !== "terminal" ||
      current === null ||
      previous === null ||
      previous === current ||
      terminalWarningSilencedRef.current ||
      !terminalProfiles.includes(current)
    ) {
      return
    }
    const profile = mikrotik.profiles.find((item) => item.id === current)
    if (profile !== undefined) setTerminalSwitchWarning(profile)
  }, [mikrotik.selectedProfile?.id, activeTab, terminalProfiles, mikrotik.profiles])

  // Terminal slots: every activated profile keeps a mounted panel; the
  // selected profile always gets one (ephemeral until it connects).
  const terminalSlotIds = new Set<number>(terminalProfiles)
  if (mikrotik.selectedProfile !== null) terminalSlotIds.add(mikrotik.selectedProfile.id)

  async function handleDeleteSession(id: number): Promise<void> {
    if (await confirm("Delete this MikroTik session?")) {
      await mikrotik.deleteSession(id)
    }
  }

  async function handleDeleteSessions(ids: readonly number[]): Promise<void> {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} sessions?`))) return
    await Promise.all(ids.map((id) => mikrotik.deleteSession(id)))
  }

  return (
    <section className={styles.view} data-testid="mikrotik-view">
      <ViewHeader title={<h1>MikroTik</h1>} subtitle={mikrotik.selectedProfile?.name ?? "no profile selected"}>
        <div className={`${styles.controls} vk-view-actions`}>
          <div className={styles.field}>
            <label htmlFor="mikrotik-profile">Profile</label>
            <select
              id="mikrotik-profile"
              value={mikrotik.selectedProfile?.id ?? ""}
              onChange={(event) => {
                const profile = mikrotik.profiles.find(
                  (item) => String(item.id) === event.currentTarget.value,
                ) ?? null
                mikrotik.selectProfile(profile)
              }}
            >
              <option value="">Select a profile</option>
              {mikrotik.profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>{profile.name}</option>
              ))}
            </select>
          </div>
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => mikrotik.selectedProfile !== null && void mikrotik.start(mikrotik.selectedProfile.id)} disabled={mikrotik.selectedDevice?.running ?? false}>Connect</Button>
          </div>
        </div>
      </ViewHeader>

      {mikrotik.error ? <div className={styles.banner}>{mikrotik.error}</div> : null}

      {mikrotik.activeDevices.length > 0 ? (
        <div className={styles.deviceStrip} role="list" aria-label="Active devices" data-testid="mikrotik-device-strip">
          {mikrotik.activeDevices.map((device) => {
            const profile = mikrotik.profiles.find((item) => item.id === device.profileId)
            const name = profile?.name ?? `profile ${device.profileId}`
            const selected = device.profileId === mikrotik.selectedProfile?.id
            return (
              <div
                key={device.profileId}
                role="listitem"
                className={`${styles.deviceChip}${selected ? ` ${styles.deviceChipSelected}` : ""}`}
                data-testid={`mikrotik-device-${device.profileId}`}
              >
                <button
                  type="button"
                  className={styles.deviceSelect}
                  aria-pressed={selected}
                  onClick={() => profile !== undefined && mikrotik.selectProfile(profile)}
                >
                  <Live />
                  <span>{name}</span>
                  {terminalProfiles.includes(device.profileId) ? (
                    <span
                      className={styles.deviceChipTerm}
                      style={{ "--terminal-accent": terminalAccentFor(device.profileId) } as CSSProperties}
                      title="An SSH terminal is open for this device"
                    >
                      term
                    </span>
                  ) : null}
                  {!device.attached ? <span className={styles.deviceDetached} title="Live, but this view is not receiving its data (the view was reloaded). Reconnect to resume.">detached</span> : null}
                </button>
                <Button
                  variant="outline-danger"
                  onClick={() => void mikrotik.stop(device.profileId)}
                  disabled={!device.attached}
                >
                  Disconnect
                </Button>
              </div>
            )
          })}
        </div>
      ) : null}

      {detachedProfileId !== null ? (
        <div className={styles.banner}>
          This session is live but this view is not receiving its data (the view was reloaded).
          {" "}
          <Button variant="outline-accent" onClick={() => void mikrotik.reconnect(detachedProfileId)}>Reconnect</Button>
        </div>
      ) : null}

      <div className={styles.tabBar} role="tablist" aria-label="MikroTik sections">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            id={`mikrotik-tab-${tab.id}`}
            type="button"
            role="tab"
            aria-controls={`mikrotik-panel-${tab.id}`}
            aria-selected={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      <section className={styles.tabPanel} id="mikrotik-panel-profiles" role="tabpanel" aria-labelledby="mikrotik-tab-profiles" hidden={activeTab !== "profiles"}>
        <MikrotikProfilePanel activeProfileId={mikrotik.selectedProfile?.id ?? null} onProfilesChanged={() => void mikrotik.refreshProfiles()} />
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-backups" role="tabpanel" aria-labelledby="mikrotik-tab-backups" hidden={activeTab !== "backups"}>
        <div className={styles.tabStack}>
          <MikrotikBackupPanel
            profileId={mikrotik.selectedProfile?.id ?? null}
            onCreated={() => setBackupsRefreshKey((current) => current + 1)}
          />
          <MikrotikBackupLibrary refreshKey={backupsRefreshKey} />
        </div>
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-system" role="tabpanel" aria-labelledby="mikrotik-tab-system" hidden={activeTab !== "system"}>
        <div className={styles.content}>
          <div className={styles.livePane}>
            <MikrotikStatusCards snapshot={mikrotik.latestSnapshot} metadata={mikrotik.latestSnapshot?.resources ?? metadata} />
            <MikrotikVersionPanel profileId={mikrotik.selectedProfile?.id ?? null} updateStatus={mikrotik.updateStatus} firmwareStatus={mikrotik.firmwareStatus} />
            <MikrotikGraphs snapshots={mikrotik.snapshotHistory} rateSeries={mikrotik.rateSeries} selectedInterface={selectedInterface} charts={SYSTEM_CHARTS} />
          </div>
          <div className={styles.historyPane}>
            <MikrotikSessionPanel sessions={mikrotik.sessions} disabled={mikrotik.selectedDevice?.running ?? false} onOpen={mikrotik.loadSession} onDelete={(id) => void handleDeleteSession(id)} onDeleteMany={(ids) => void handleDeleteSessions(ids)} />
          </div>
        </div>
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-interfaces" role="tabpanel" aria-labelledby="mikrotik-tab-interfaces" hidden={activeTab !== "interfaces"}>
        <div className={styles.tabStack}>
          <p className={styles.hint}>Select an interface row to update the rate graph below.</p>
          <MikrotikInterfaceTable interfaces={interfaces} onSelectInterface={setSelectedInterfaceName} />
          <p className={styles.selected} data-testid="mikrotik-selected-interface">Selected interface: {selectedInterface ?? "none"}</p>
          <MikrotikGraphs snapshots={mikrotik.snapshotHistory} rateSeries={mikrotik.rateSeries} selectedInterface={selectedInterface} charts={INTERFACE_CHARTS} />
        </div>
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-vlans" role="tabpanel" aria-labelledby="mikrotik-tab-vlans" hidden={activeTab !== "vlans"}>
        <MikrotikVlanPanel vlans={mikrotik.vlans} bridgeVlans={mikrotik.bridgeVlans} />
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-logs" role="tabpanel" aria-labelledby="mikrotik-tab-logs" hidden={activeTab !== "logs"}>
        <MikrotikLogsPanel profileId={mikrotik.selectedProfile?.id ?? null} />
      </section>

      <section className={styles.tabPanel} id="mikrotik-panel-terminal" role="tabpanel" aria-labelledby="mikrotik-tab-terminal" hidden={activeTab !== "terminal"}>
        {mikrotik.selectedProfile === null ? (
          <p className={styles.hint} data-testid="mikrotik-terminal-empty">Select a profile to open a terminal.</p>
        ) : null}
        {[...terminalSlotIds].map((profileId) => {
          const profile = mikrotik.profiles.find((item) => item.id === profileId)
          if (profile === undefined) return null
          return (
            <div
              key={profileId}
              data-testid={`mikrotik-terminal-slot-${profileId}`}
              hidden={profileId !== mikrotik.selectedProfile?.id}
            >
              <MikrotikTerminalPanel
                profileId={profileId}
                profileName={profile.name}
                profileHost={profile.host}
                onActivated={activateTerminal}
                onDeactivated={deactivateTerminal}
              />
            </div>
          )
        })}
      </section>
      {terminalSwitchWarning !== null ? (
        <TerminalSwitchWarning
          profileName={terminalSwitchWarning.name}
          profileHost={terminalSwitchWarning.host}
          accent={terminalAccentFor(terminalSwitchWarning.id)}
          onDismiss={(silence) => {
            if (silence) terminalWarningSilencedRef.current = true
            setTerminalSwitchWarning(null)
          }}
        />
      ) : null}
      {confirmDialog}
    </section>
  )
}
