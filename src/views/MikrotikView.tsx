import { useEffect, useMemo, useState } from "react"
import { MikrotikBackupPanel } from "../components/MikrotikBackupPanel"
import { MikrotikBackupLibrary } from "../components/MikrotikBackupLibrary"
import { MikrotikGraphs } from "../components/MikrotikGraphs"
import { MikrotikInterfaceTable } from "../components/MikrotikInterfaceTable"
import { MikrotikProfilePanel } from "../components/MikrotikProfilePanel"
import { MikrotikSessionPanel } from "../components/MikrotikSessionPanel"
import { MikrotikStatusCards } from "../components/MikrotikStatusCards"
import { MikrotikVersionPanel } from "../components/MikrotikVersionPanel"
import { MikrotikVlanPanel } from "../components/MikrotikVlanPanel"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { useMikrotik } from "../hooks/useMikrotik"

import styles from "./MikrotikView.module.css"

const TABS = [
  { id: "profiles", label: "Profiles" },
  { id: "backups", label: "Backups" },
  { id: "system", label: "System" },
  { id: "interfaces", label: "Interfaces" },
  { id: "vlans", label: "VLANs" },
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
  const interfaces = mikrotik.latestSnapshot?.interfaces ?? []
  const selectedInterface = useMemo(() => {
    if (interfaces.some((item) => item.name === selectedInterfaceName)) {
      return selectedInterfaceName
    }
    return interfaces[0]?.name ?? null
  }, [interfaces, selectedInterfaceName])
  const metadata = mikrotik.loadedSession?.session ?? null

  useEffect(() => {
    setSelectedInterfaceName((current) =>
      interfaces.some((item) => item.name === current) ? current : (interfaces[0]?.name ?? null),
    )
  }, [interfaces])

  async function handleDeleteSession(id: number): Promise<void> {
    if (await confirm("Delete this MikroTik session?")) {
      await mikrotik.deleteSession(id)
    }
  }

  return (
    <section className={styles.view} data-testid="mikrotik-view">
      <header className={styles.header}>
        <h1>MikroTik monitoring</h1>
      </header>

      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor="mikrotik-profile">Profile</label>
          <select
            id="mikrotik-profile"
            value={mikrotik.selectedProfile?.id ?? ""}
            disabled={mikrotik.running}
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
          <button type="button" onClick={() => void mikrotik.start()} disabled={mikrotik.running}>Start</button>
          <button type="button" onClick={() => void mikrotik.stop()} disabled={!mikrotik.running}>Stop</button>
        </div>
      </div>

      {mikrotik.error ? <div className={styles.banner}>{mikrotik.error}</div> : null}

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
        <MikrotikProfilePanel activeProfileId={mikrotik.selectedProfile?.id ?? null} />
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
            <MikrotikSessionPanel sessions={mikrotik.sessions} disabled={mikrotik.running} onOpen={mikrotik.loadSession} onDelete={(id) => void handleDeleteSession(id)} />
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
      {confirmDialog}
    </section>
  )
}
