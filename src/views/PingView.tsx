import { useEffect, useRef } from "react"
import { validateTarget } from "../lib/validate"
import { DEFAULT_PAYLOAD_SIZE, MAX_PAYLOAD_SIZE } from "../lib/constants"
import { FAMILIES, type Family } from "../lib/types"
import { usePingSession } from "../hooks/usePingSession"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { PingGraphs } from "../components/PingGraphs"
import { PingSessionPanel } from "../components/PingSessionPanel"
import { PingStatsTable } from "../components/PingStatsTable"
import {
  Button,
  Chip,
  Diamond,
  Live,
  SectionHeader,
  ViewHeader,
} from "../components/ui/ui"

import styles from "./PingView.module.css"

type PingViewProps = {
  onClose?: () => void
  initialSessionId?: number
}

export default function PingView({ onClose, initialSessionId }: PingViewProps) {
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

  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const handleDeleteSession = async (id: number) => {
    if (await confirm("Delete this session?")) {
      await deleteSession(id)
    }
  }
  const handleDeleteSessions = async (ids: readonly number[]) => {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} sessions?`))) return
    await Promise.all(ids.map((id) => deleteSession(id)))
  }

  const loadedInitial = useRef(false)
  useEffect(() => {
    if (initialSessionId === undefined) return
    if (loadedInitial.current) return
    loadedInitial.current = true
    void openSession(initialSessionId)
  }, [initialSessionId, openSession])

  const validation = validateTarget(target)
  const canStart = validation.ok && !isRunning
  const resolvedIp =
    viewMode === "past" ? pastSession?.session.resolvedIp : startInfo?.resolvedIp
  const answers =
    viewMode === "past" ? [pastSession?.session.resolvedIp] : startInfo?.answers
  const dfDisabled = family === "v6" || isRunning
  const displaySnapshot = viewMode === "past" ? snapshot : snapshot
  const displayTarget =
    target || pastSession?.session.targetInput || "Ready for a target"

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
      <div className={styles.cardHeader}>
        <div className={styles.sessionIdentity}>
          <SectionHeader
            title={viewMode === "past" ? "Saved ping" : "Ping session"}
            aside={displayTarget}
          />
        </div>
        {onClose !== undefined && (
          <Button
            small
            className={styles.closeButton}
            onClick={onClose}
            aria-label="Close"
            data-testid="close-ping"
          >
            <span aria-hidden="true">×</span>
          </Button>
        )}
      </div>

      <ViewHeader>
        <div className={styles.controls}>
          <div className={`${styles.field} ${styles.targetField}`}>
            <Chip
              label={<label htmlFor="ping-target">Target</label>}
              value={
                <input
                  className={styles.chipInput}
                  id="ping-target"
                  type="text"
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                  placeholder="example.com, 192.168.1.1, ::1"
                  disabled={isRunning}
                  data-testid="ping-target"
                />
              }
              aside={
                resolvedIp !== undefined ? (
                  <span className={styles.resolved} data-testid="resolved-info">
                    <span className={styles.resolvedIp} data-testid="resolved-ip">
                      {resolvedIp}
                    </span>
                    {answers !== undefined && answers.length > 1 && (
                      <span className={styles.answers}>
                        {answers.filter(Boolean).join(", ")}
                      </span>
                    )}
                  </span>
                ) : undefined
              }
            />
            {!validation.ok && target.trim().length > 0 && (
              <span className={styles.inlineError} data-testid="ping-target-error">
                {validation.error}
              </span>
            )}
          </div>

          <div className={styles.field}>
            <Chip
              label={<label htmlFor="ping-family">Family</label>}
              value={
                <select
                  className={styles.chipSelect}
                  id="ping-family"
                  value={family}
                  onChange={(event) =>
                    handleFamilyChange(event.target.value as Family)
                  }
                  disabled={isRunning}
                  data-testid="ping-family"
                >
                  {FAMILIES.map((familyOption) => (
                    <option key={familyOption} value={familyOption}>
                      {familyOption === "auto"
                        ? "Auto"
                        : familyOption === "v4"
                          ? "IPv4"
                          : "IPv6"}
                    </option>
                  ))}
                </select>
              }
            />
          </div>

          <div className={styles.field}>
            <Chip
              label={<label htmlFor="ping-payload">Bytes</label>}
              value={
                <input
                  className={styles.chipNumber}
                  id="ping-payload"
                  type="number"
                  min={1}
                  max={MAX_PAYLOAD_SIZE}
                  value={payloadSize}
                  onChange={(event) =>
                    handlePayloadSizeChange(event.target.value)
                  }
                  disabled={isRunning}
                  data-testid="ping-payload"
                />
              }
            />
          </div>

          <div className={`${styles.field} ${styles.checkboxField}`}>
            <Chip
              label="DF"
              value={
                <label className={styles.checkboxControl} htmlFor="ping-df">
                  <input
                    id="ping-df"
                    type="checkbox"
                    checked={dontFragment}
                    onChange={(event) => setDontFragment(event.target.checked)}
                    disabled={dfDisabled}
                    data-testid="ping-df"
                  />
                  <span>Don't fragment</span>
                </label>
              }
            />
          </div>

          <div className={styles.actions}>
            {isRunning && <Live />}
            <Button
              variant="primary"
              onClick={handleStart}
              disabled={!canStart}
              data-testid="ping-start"
            >
              Start
            </Button>
            <Button
              variant="outline-accent"
              onClick={() => void stop()}
              disabled={!isRunning}
              data-testid="ping-stop"
            >
              Stop
            </Button>
          </div>
        </div>
      </ViewHeader>

      <div className={styles.body}>
        {pausedError.length > 0 && (
          <div className={styles.pauseBanner} data-testid="pause-banner">
            <div className={styles.pauseMessage} data-testid="pause-message">
              Session paused: {pausedError}
            </div>
            <Button
              small
              variant="outline-danger"
              onClick={() => void retry()}
              disabled={pausedError.length === 0}
              data-testid="retry-fallback"
            >
              Retry with fallback
            </Button>
          </div>
        )}

        {(status.length > 0 || error.length > 0) && (
          <div className={styles.banner}>
            {status.length > 0 && (
              <div className={styles.statusLine}>
                <Diamond small />
                <span className={styles.status} data-testid="ping-status">
                  {status}
                </span>
              </div>
            )}
            {error.length > 0 && (
              <div className={styles.errorLine}>
                <Diamond small color="var(--danger)" />
                <span className={styles.error} data-testid="ping-error">
                  {error}
                </span>
              </div>
            )}
          </div>
        )}

        <div className={styles.dataGrid}>
          <div className={styles.tableWrapper}>
            <PingStatsTable rows={tableRows} snapshot={displaySnapshot} />
          </div>

          <div className={styles.graphsWrapper}>
            <PingGraphs probes={allProbes} />
          </div>
        </div>

        <div className={styles.sessionsWrapper}>
          <PingSessionPanel
            sessions={sessions}
            disabled={isRunning}
            onOpen={openSession}
            onDelete={handleDeleteSession}
            onDeleteMany={handleDeleteSessions}
          />
        </div>
      </div>
      {confirmDialog}
    </section>
  )
}
