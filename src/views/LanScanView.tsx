import { useState } from "react"
import type { ScanHostDto } from "../lib/types"
import { useLanScan } from "../hooks/useLanScan"
import { usePortScanConsent } from "../hooks/usePortScanConsent"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { LanScanTable } from "../components/LanScanTable"
import { ScanSessionPanel } from "../components/ScanSessionPanel"
import {
  Button,
  Card,
  Chip,
  Live,
  StatusBar,
  ViewHeader,
} from "../components/ui/ui"

import styles from "./LanScanView.module.css"

function interfaceTitle(iface: {
  readonly name: string
  readonly ipv4: string
  readonly prefixLen: number
}): string {
  const prefix = iface.prefixLen ?? 24
  return `${iface.name} · ${iface.ipv4}/${prefix}`
}

function estimateHostCount(cidr: string): number {
  const match = cidr.trim().match(/\/(\d{1,2})$/)
  if (match) {
    const prefix = parseInt(match[1], 10)
    if (prefix >= 0 && prefix <= 30) {
      return Math.max(0, Math.pow(2, 32 - prefix) - 2)
    }
  }
  return 254
}

function downloadScanCsv(rows: readonly ScanHostDto[], cidr: string): void {
  const records = [
    ["IP", "MAC", "Vendor", "Hostname", "Open ports", "RTT", "Last seen"],
    ...rows.map((row) => [
      row.ip,
      row.mac ?? "",
      row.vendor ?? "",
      row.hostname ?? "",
      row.openPorts.map((port) => `${port.port} ${port.service}`).join(" "),
      "",
      row.at,
    ]),
  ]
  const csv = records
    .map((record) =>
      record
        .map((value) => `"${value.replaceAll('"', '""')}"`)
        .join(","),
    )
    .join("\n")
  const url = URL.createObjectURL(new Blob([csv], { type: "text/csv;charset=utf-8" }))
  const anchor = document.createElement("a")
  anchor.href = url
  anchor.download = `verkkokyyla-scan-${cidr.replace("/", "-")}.csv`
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}

export default function LanScanView() {
  const {
    interfaces,
    selectedInterface,
    selectInterface,
    cidr,
    setCidr,
    tcpFallback,
    setTcpFallback,
    portsEnabled,
    setPortsEnabled,
    isRunning,
    error,
    status,
    progress,
    hosts,
    pastScans,
    viewMode,
    pastScan,
    start,
    stop,
    openScan,
    deleteScan,
  } = useLanScan()

  const { hasConsented, recordConsent } = usePortScanConsent()
  const [showConsentDialog, setShowConsentDialog] = useState(false)
  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const handleDeleteScan = async (id: number) => {
    if (await confirm("Delete this scan?")) {
      await deleteScan(id)
    }
  }
  const handleDeleteScans = async (ids: readonly number[]) => {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} scans?`))) return
    await Promise.all(ids.map((id) => deleteScan(id)))
  }

  const canStart =
    selectedInterface !== null && cidr.trim().length > 0 && !isRunning
  const displayHosts = viewMode === "past" ? pastScan?.hosts ?? [] : hosts
  const estimatedHosts = estimateHostCount(cidr)
  const estimatedProbes = estimatedHosts * 25

  const handlePortsChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const checked = event.target.checked
    if (checked && !hasConsented) {
      setPortsEnabled(true)
      setShowConsentDialog(true)
    } else {
      setPortsEnabled(checked)
    }
  }

  const handleCancelConsent = () => {
    setPortsEnabled(false)
    setShowConsentDialog(false)
  }

  const handleConfirmConsent = () => {
    recordConsent()
    setShowConsentDialog(false)
  }

  return (
    <section className={styles.view} data-testid="lan-scan-view">
      <h1 className={styles.srOnly}>Network scanner</h1>
      <ViewHeader>
        <div className={styles.subnetControl}>
          <Chip
            label={<label htmlFor="lan-cidr">Subnet</label>}
            value={
              <input
                id="lan-cidr"
                className={styles.cidrInput}
                type="text"
                value={cidr}
                onChange={(event) => setCidr(event.target.value)}
                placeholder="192.168.1.0/24"
                disabled={isRunning}
                data-testid="lan-cidr"
              />
            }
          />
        </div>

        <Chip
          label={<label htmlFor="lan-interface">Iface</label>}
          value={
            <select
              id="lan-interface"
              className={styles.interfaceSelect}
              aria-label="Network interface"
              value={selectedInterface?.name ?? ""}
              onChange={(event) => {
                const iface = interfaces.find(
                  (item) => item.name === event.target.value,
                )
                if (iface !== undefined) selectInterface(iface)
              }}
              disabled={isRunning}
              data-testid="lan-interface"
            >
              {interfaces.length === 0 && (
                <option value="">No interfaces found</option>
              )}
              {interfaces.map((iface) => (
                <option key={iface.name} value={iface.name} title={interfaceTitle(iface)}>
                  {iface.name}
                </option>
              ))}
            </select>
          }
        />

        <div className={styles.optionControl}>
          <input
            id="lan-ports-enabled"
            className={styles.optionInput}
            type="checkbox"
            checked={portsEnabled}
            onChange={handlePortsChange}
            disabled={isRunning}
            data-testid="lan-ports-enabled"
          />
          <Chip
            label={<label htmlFor="lan-ports-enabled">ports</label>}
            value={portsEnabled ? "top 25" : "off"}
          />
        </div>

        <div className={styles.optionControl}>
          <input
            id="lan-tcp-fallback"
            className={styles.optionInput}
            type="checkbox"
            checked={tcpFallback}
            onChange={(event) => setTcpFallback(event.target.checked)}
            disabled={isRunning}
            data-testid="lan-tcp-fallback"
          />
          <Chip
            label={<label htmlFor="lan-tcp-fallback">mode</label>}
            value={tcpFallback ? "ARP + TCP" : "ARP only"}
          />
        </div>

        <div className={`vk-view-actions ${styles.actions}`}>
          <Button
            variant="primary"
            className={isRunning ? styles.hiddenAction : ""}
            type="button"
            onClick={() => void start()}
            disabled={!canStart}
            data-testid="lan-start"
          >
            Start scan
          </Button>
          <Button
            type="button"
            onClick={() => downloadScanCsv(displayHosts, cidr)}
            disabled={displayHosts.length === 0}
          >
            Export CSV
          </Button>
          <Button
            variant="outline-accent"
            className={!isRunning ? styles.hiddenAction : ""}
            type="button"
            onClick={() => void stop()}
            disabled={!isRunning}
            data-testid="lan-stop"
          >
            Stop scan
          </Button>
        </div>
      </ViewHeader>

      {portsEnabled && (
        <div
          className={`${styles.banner} ${styles.warning}`}
          data-testid="lan-port-scan-warning"
        >
          Scanning sends TCP connection attempts to devices on this network. Network
          monitors, firewalls, and IDS/SIEM tools may log or alert on this activity.
        </div>
      )}

      {error.length > 0 && (
        <div className={`${styles.banner} ${styles.error}`} data-testid="lan-error">
          {error}
        </div>
      )}

      {progress !== null && progress.total > 0 && (
        <div className={styles.progress} data-testid="lan-progress">
          <progress
            max={progress.total}
            value={progress.done}
            aria-label={`${progress.done} of ${progress.total} hosts probed`}
          >
            Scanned {progress.done} of {progress.total} hosts
          </progress>
          <span className={styles.progressCount}>
            {progress.done} / {progress.total} probed
          </span>
          <span className={styles.upCount}>{displayHosts.length} up</span>
          <span className={styles.srOnly}>
            Scanned {progress.done} of {progress.total} hosts
          </span>
        </div>
      )}

      <div className={styles.workspace}>
        <Card className={styles.devicePanel}>
          <LanScanTable rows={displayHosts} />
          {isRunning && (
            <div className={styles.probingRow}>
              <Live>probing …</Live>
            </div>
          )}
        </Card>
        <ScanSessionPanel
          scans={pastScans}
          disabled={isRunning}
          onOpen={openScan}
          onDelete={handleDeleteScan}
          onDeleteMany={handleDeleteScans}
        />
      </div>

      <StatusBar>
        {isRunning && <Live>scanning</Live>}
        {!isRunning && status.length === 0 && (
          <span className={styles.readyStatus}>ready</span>
        )}
        {status.length > 0 && (
          <span className={styles.scanStatus} data-testid="lan-status">
            {status}
          </span>
        )}
        <span>{displayHosts.length} up</span>
        <span>{pastScans.length} saved scans</span>
        <span className="vk-statusbar-right">
          {viewMode === "past" ? "saved scan" : selectedInterface?.name ?? "no interface"}
        </span>
      </StatusBar>

      {showConsentDialog && (
        <div
          className={styles.modalOverlay}
          data-testid="lan-port-scan-consent-dialog"
          onClick={(event) => {
            if (event.target === event.currentTarget) {
              handleCancelConsent()
            }
          }}
        >
          <div className={styles.modal} role="dialog" aria-modal="true">
            <h2>Network scan warning</h2>
            <div className={styles.modalBody}>
              <p>
                <strong>CIDR:</strong> {cidr}
              </p>
              <p>
                <strong>Estimated host count:</strong> up to {estimatedHosts} hosts
              </p>
              <p>
                <strong>Ports:</strong> 25
              </p>
              <p>
                <strong>Estimated probes:</strong> up to {estimatedProbes}
              </p>
              <p>
                <strong>Mode:</strong> TCP connect-only
              </p>
              <p>
                <strong>Rate:</strong> ~10 attempts/s
              </p>
              <p>
                <strong>Max concurrency:</strong> 32
              </p>
            </div>
            <div className={styles.modalActions}>
              <Button type="button" small onClick={handleCancelConsent}>
                Cancel
              </Button>
              <Button
                type="button"
                small
                variant="primary"
                onClick={handleConfirmConsent}
              >
                Confirm
              </Button>
            </div>
          </div>
        </div>
      )}
      {confirmDialog}
    </section>
  )
}
