import { useState } from "react"
import { useLanScan } from "../hooks/useLanScan"
import { usePortScanConsent } from "../hooks/usePortScanConsent"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { LanScanTable } from "../components/LanScanTable"
import { ScanSessionPanel } from "../components/ScanSessionPanel"

import styles from "./LanScanView.module.css"

function formatInterfaceOption(iface: {
  readonly name: string
  readonly ipv4: string
  readonly prefixLen: number
}): string {
  const prefix = iface.prefixLen ?? 24
  return `${iface.name} — ${iface.ipv4}/${prefix}`
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
      <header className={styles.header}>
        <h1>Network scanner</h1>
      </header>

      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor="lan-interface">Interface</label>
          <select
            id="lan-interface"
            value={selectedInterface?.name ?? ""}
            onChange={(event) => {
              const iface = interfaces.find((item) => item.name === event.target.value)
              if (iface !== undefined) {
                selectInterface(iface)
              }
            }}
            disabled={isRunning}
            data-testid="lan-interface"
          >
            {interfaces.length === 0 && (
              <option value="">No interfaces found</option>
            )}
            {interfaces.map((iface) => (
              <option key={iface.name} value={iface.name}>
                {formatInterfaceOption(iface)}
              </option>
            ))}
          </select>
        </div>

        <div className={styles.field}>
          <label htmlFor="lan-cidr">CIDR</label>
          <input
            id="lan-cidr"
            type="text"
            value={cidr}
            onChange={(event) => setCidr(event.target.value)}
            placeholder="192.168.1.0/24"
            disabled={isRunning}
            data-testid="lan-cidr"
          />
        </div>

        <div className={`${styles.field} ${styles.checkbox}`}>
          <label htmlFor="lan-tcp-fallback">
            <input
              id="lan-tcp-fallback"
              type="checkbox"
              checked={tcpFallback}
              onChange={(event) => setTcpFallback(event.target.checked)}
              disabled={isRunning}
              data-testid="lan-tcp-fallback"
            />
            TCP fallback
          </label>
        </div>

        <div className={`${styles.field} ${styles.checkbox}`}>
          <label htmlFor="lan-ports-enabled">
            <input
              id="lan-ports-enabled"
              type="checkbox"
              checked={portsEnabled}
              onChange={handlePortsChange}
              disabled={isRunning}
              data-testid="lan-ports-enabled"
            />
            Scan common ports
          </label>
        </div>

        <div className={styles.actions}>
          <button
            type="button"
            onClick={() => void start()}
            disabled={!canStart}
            data-testid="lan-start"
          >
            Start
          </button>
          <button
            type="button"
            onClick={() => void stop()}
            disabled={!isRunning}
            data-testid="lan-stop"
          >
            Stop
          </button>
        </div>
      </div>

      {portsEnabled && (
        <div
          className={`${styles.banner} ${styles.warning}`}
          data-testid="lan-port-scan-warning"
        >
          Scanning sends TCP connection attempts to devices on this network. Network
          monitors, firewalls, and IDS/SIEM tools may log or alert on this activity.
        </div>
      )}

      {(status.length > 0 || error.length > 0) && (
        <div className={styles.banner}>
          {status.length > 0 && (
            <div className={styles.status} data-testid="lan-status">
              {status}
            </div>
          )}
          {error.length > 0 && (
            <div className={styles.error} data-testid="lan-error">
              {error}
            </div>
          )}
        </div>
      )}

      {progress !== null && progress.total > 0 && (
        <div className={styles.progress} data-testid="lan-progress">
          <progress max={progress.total} value={progress.done}>
            Scanned {progress.done} of {progress.total} hosts
          </progress>
          <span>
            Scanned {progress.done} of {progress.total} hosts
          </span>
        </div>
      )}

      <div className={styles.content}>
        <div className={styles.livePane}>
          {displayHosts.length === 0 ? (
            <p className={styles.empty}>No hosts discovered yet.</p>
          ) : (
            <LanScanTable rows={displayHosts} />
          )}
        </div>
        <div className={styles.historyPane}>
          <ScanSessionPanel
            scans={pastScans}
            disabled={isRunning}
            onOpen={openScan}
            onDelete={handleDeleteScan}
          />
        </div>
      </div>

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
              <button type="button" onClick={handleCancelConsent}>
                Cancel
              </button>
              <button type="button" onClick={handleConfirmConsent}>
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}
      {confirmDialog}
    </section>
  )
}
