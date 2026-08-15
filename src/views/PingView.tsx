import { validateTarget } from "../lib/validate"
import { DEFAULT_PAYLOAD_SIZE, MAX_PAYLOAD_SIZE } from "../lib/constants"
import { FAMILIES, type Family } from "../lib/types"
import { usePingSession } from "../hooks/usePingSession"
import { PingGraphs } from "../components/PingGraphs"
import { PingSessionPanel } from "../components/PingSessionPanel"
import { PingStatsTable } from "../components/PingStatsTable"

import styles from "./PingView.module.css"

export default function PingView() {
  const {
    target,
    setTarget,
    family,
    setFamily,
    payloadSize,
    setPayloadSize,
    dontFragment,
    setDontFragment,
    isRunning,
    error,
    pausedError,
    status,
    startInfo,
    snapshot,
    tableRows,
    allProbes,
    sessions,
    viewMode,
    pastSession,
    start,
    stop,
    retry,
    openSession,
    deleteSession,
  } = usePingSession()

  const validation = validateTarget(target)
  const canStart = validation.ok && !isRunning
  const resolvedIp =
    viewMode === "past" ? pastSession?.session.resolvedIp : startInfo?.resolvedIp
  const answers =
    viewMode === "past" ? [pastSession?.session.resolvedIp] : startInfo?.answers
  const dfDisabled = family === "v6" || isRunning
  const displaySnapshot = viewMode === "past" ? snapshot : snapshot

  const handleStart = () => {
    if (!canStart) return
    void start()
  }

  const handleFamilyChange = (value: Family) => {
    if (isRunning) return
    setFamily(value)
    if (value === "v6") {
      setDontFragment(false)
    }
  }

  const handlePayloadSizeChange = (value: string) => {
    const parsed = Number.parseInt(value, 10)
    if (Number.isNaN(parsed)) {
      setPayloadSize(DEFAULT_PAYLOAD_SIZE)
      return
    }
    setPayloadSize(Math.max(1, Math.min(parsed, MAX_PAYLOAD_SIZE)))
  }

  return (
    <section className={styles.view}>
      <h1>Ping</h1>

      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor="ping-target">Target</label>
          <input
            id="ping-target"
            type="text"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            placeholder="example.com, 192.168.1.1, ::1"
            disabled={isRunning}
            data-testid="ping-target"
          />
          {!validation.ok && target.trim().length > 0 && (
            <span className={styles.inlineError} data-testid="ping-target-error">
              {validation.error}
            </span>
          )}
        </div>

        <div className={styles.field}>
          <label htmlFor="ping-family">Family</label>
          <select
            id="ping-family"
            value={family}
            onChange={(e) => handleFamilyChange(e.target.value as Family)}
            disabled={isRunning}
            data-testid="ping-family"
          >
            {FAMILIES.map((f) => (
              <option key={f} value={f}>
                {f === "auto" ? "Auto" : f === "v4" ? "IPv4" : "IPv6"}
              </option>
            ))}
          </select>
        </div>

        <div className={styles.field}>
          <label htmlFor="ping-payload">Packet size</label>
          <input
            id="ping-payload"
            type="number"
            min={1}
            max={MAX_PAYLOAD_SIZE}
            value={payloadSize}
            onChange={(e) => handlePayloadSizeChange(e.target.value)}
            disabled={isRunning}
            data-testid="ping-payload"
          />
        </div>

        <div className={`${styles.field} ${styles.checkboxField}`}>
          <label htmlFor="ping-df">
            <input
              id="ping-df"
              type="checkbox"
              checked={dontFragment}
              onChange={(e) => setDontFragment(e.target.checked)}
              disabled={dfDisabled}
              data-testid="ping-df"
            />
            Don't fragment
          </label>
        </div>

        <div className={styles.actions}>
          <button
            type="button"
            onClick={handleStart}
            disabled={!canStart}
            data-testid="ping-start"
          >
            Start
          </button>
          <button
            type="button"
            onClick={() => void stop()}
            disabled={!isRunning}
            data-testid="ping-stop"
          >
            Stop
          </button>
        </div>
      </div>

      {pausedError.length > 0 && (
        <div className={styles.pauseBanner} data-testid="pause-banner">
          <div className={styles.pauseMessage} data-testid="pause-message">
            Session paused: {pausedError}
          </div>
          <button
            type="button"
            onClick={() => void retry()}
            disabled={pausedError.length === 0}
            data-testid="retry-fallback"
          >
            Retry with fallback
          </button>
        </div>
      )}

      {(status.length > 0 || error.length > 0 || resolvedIp !== undefined) && (
        <div className={styles.banner}>
          {status.length > 0 && (
            <div className={styles.status} data-testid="ping-status">
              {status}
            </div>
          )}
          {error.length > 0 && (
            <div className={styles.error} data-testid="ping-error">
              {error}
            </div>
          )}
          {resolvedIp !== undefined && (
            <div className={styles.resolved} data-testid="resolved-info">
              <span>Resolved IP: </span>
              <span className={styles.resolvedIp} data-testid="resolved-ip">
                {resolvedIp}
              </span>
              {answers !== undefined && answers.length > 1 && (
                <span className={styles.answers}>
                  ({answers.filter(Boolean).join(", ")})
                </span>
              )}
            </div>
          )}
        </div>
      )}

      <div className={styles.tableWrapper}>
        <PingStatsTable rows={tableRows} snapshot={displaySnapshot} />
      </div>

      <div className={styles.graphsWrapper}>
        <PingGraphs probes={allProbes} />
      </div>

      <div className={styles.sessionsWrapper}>
        <PingSessionPanel
          sessions={sessions}
          disabled={isRunning}
          onOpen={openSession}
          onDelete={deleteSession}
        />
      </div>
    </section>
  )
}
