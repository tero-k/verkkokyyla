import { useTraceroute } from "../hooks/useTraceroute"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { FAMILIES, type Family } from "../lib/types"
import { TraceSessionPanel } from "../components/TraceSessionPanel"
import { TracerouteComparison } from "../components/TracerouteComparison"
import { TracerouteTable } from "../components/TracerouteTable"
import {
  Button,
  Card,
  Chip,
  Live,
  Stat,
  StatusBar,
  ViewHeader,
} from "../components/ui/ui"
import type { TraceHopRow } from "../lib/types"

import styles from "./TracerouteView.module.css"

const MAX_HOPS = 30
const PROBES_PER_HOP = 3

type TraceMetrics = {
  readonly endToEndMs: number | null
  readonly worstHop: number | null
  readonly worstIncreaseMs: number | null
  readonly lossPercent: number | null
}

function familyLabel(family: Family): string {
  if (family === "auto") return "Auto"
  if (family === "v4") return "IPv4"
  return "IPv6"
}

function averageRtt(row: TraceHopRow): number | null {
  const values = [row.rtt1Ms, row.rtt2Ms, row.rtt3Ms].filter(
    (value): value is number => value !== null,
  )
  if (values.length === 0) return null
  return values.reduce((sum, value) => sum + value, 0) / values.length
}

function traceMetrics(rows: readonly TraceHopRow[]): TraceMetrics {
  let previousAverage: number | null = null
  let worstHop: number | null = null
  let worstIncreaseMs: number | null = null

  for (const row of rows) {
    const average = averageRtt(row)
    if (average === null) continue
    if (previousAverage !== null) {
      const increase = average - previousAverage
      if (increase > (worstIncreaseMs ?? 0)) {
        worstHop = row.hop
        worstIncreaseMs = increase
      }
    }
    previousAverage = average
  }

  const finalHop = rows[rows.length - 1]
  if (finalHop === undefined) {
    return { endToEndMs: null, worstHop, worstIncreaseMs, lossPercent: null }
  }

  const responses = [finalHop.rtt1Ms, finalHop.rtt2Ms, finalHop.rtt3Ms].filter(
    (value) => value !== null,
  ).length
  return {
    endToEndMs: averageRtt(finalHop),
    worstHop,
    worstIncreaseMs,
    lossPercent: ((PROBES_PER_HOP - responses) / PROBES_PER_HOP) * 100,
  }
}

function formatLatency(value: number | null): string {
  if (value === null) return "—"
  return `${value.toFixed(value < 10 ? 2 : 1)} ms`
}

function formatWorstHop(metrics: TraceMetrics): string {
  if (metrics.worstHop === null || metrics.worstIncreaseMs === null) return "—"
  return `#${metrics.worstHop} +${metrics.worstIncreaseMs.toFixed(1)} ms`
}

export default function TracerouteView() {
  const {
    target,
    setTarget,
    family,
    setFamily,
    isRunning,
    error,
    status,
    hops,
    pastTraces,
    viewMode,
    pastTrace,
    isCompareSelecting,
    compareSelection,
    compareA,
    compareB,
    compareDiff,
    startCompareSelection,
    toggleCompareSelection,
    compareSelected,
    clearCompare,
    start,
    stop,
    openTrace,
    deleteTrace,
  } = useTraceroute()

  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const handleDeleteTrace = async (id: number) => {
    if (await confirm("Delete this trace?")) {
      await deleteTrace(id)
    }
  }
  const handleDeleteTraces = async (ids: readonly number[]) => {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} traces?`))) return
    await Promise.all(ids.map((id) => deleteTrace(id)))
  }

  const canStart = target.trim().length > 0 && !isRunning
  const compareMode = viewMode === "compare" || isCompareSelecting
  const progressHop =
    isRunning || compareMode
      ? hops.length
      : pastTrace?.trace.hopCount ?? hops.length
  const metrics = traceMetrics(hops)
  const finalHop = hops[hops.length - 1]
  const resolvedIp = pastTrace?.trace.resolvedIp ?? finalHop?.address
  const lossColor =
    metrics.lossPercent === null || metrics.lossPercent === 0
      ? "var(--accent)"
      : metrics.lossPercent === 100
        ? "var(--danger)"
        : "var(--warning)"
  const insight =
    metrics.worstHop !== null && metrics.worstIncreaseMs !== null
      ? `Largest latency step begins at hop ${metrics.worstHop}, adding ${metrics.worstIncreaseMs.toFixed(1)} ms.`
      : hops.length > 0
        ? "Latency remains level across the responding route."
        : "Run a trace to profile latency and response loss at every hop."
  const showingComparison = compareMode && compareA !== null && compareB !== null

  return (
    <section className={styles.view} data-testid="traceroute-view">
      <header className={styles.header}>
        <ViewHeader>
          <h1 className={styles.srOnly}>Traceroute</h1>
          <div className={styles.targetChip}>
            <Chip
              label={<label htmlFor="trace-target">Target</label>}
              value={
                <input
                  id="trace-target"
                  className={styles.targetInput}
                  type="text"
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                  placeholder="example.com, 192.168.1.1, ::1"
                  disabled={isRunning}
                  data-testid="trace-target"
                />
              }
              aside={resolvedIp}
            />
          </div>
          <Chip label="Protocol" value="ICMP" />
          <Chip label="Probes" value={PROBES_PER_HOP} />
          <Chip label="Max hops" value={MAX_HOPS} />
          <div className={styles.familyChip}>
            <Chip
              label={<label htmlFor="trace-family">Family</label>}
              value={
                <select
                  id="trace-family"
                  className={styles.familySelect}
                  value={family}
                  onChange={(event) => setFamily(event.target.value as Family)}
                  disabled={isRunning}
                  data-testid="trace-family"
                >
                  {FAMILIES.map((value) => (
                    <option key={value} value={value}>
                      {familyLabel(value)}
                    </option>
                  ))}
                </select>
              }
            />
          </div>
          <div className={`vk-view-actions ${styles.actions}`}>
            <Button
              variant="secondary"
              onClick={() => void stop()}
              disabled={!isRunning}
              data-testid="trace-stop"
            >
              Stop
            </Button>
            <Button
              variant="primary"
              onClick={() => void start()}
              disabled={!canStart}
              data-testid="trace-start"
            >
              {hops.length > 0 || pastTrace !== null ? "Run again" : "Run trace"}
            </Button>
          </div>
        </ViewHeader>
      </header>

      <div className={`vk-view-body ${styles.body}`}>
        {(status.length > 0 || error.length > 0) && (
          <div className={styles.notices} aria-live="polite">
            {status.length > 0 && (
              <div className={styles.status} data-testid="trace-status">
                {status}
              </div>
            )}
            {error.length > 0 && (
              <div className={styles.error} data-testid="trace-error">
                {error}
              </div>
            )}
          </div>
        )}

        {!showingComparison && (
          <Card className={styles.summaryStrip}>
            <div className={styles.summaryStats}>
              <Stat label="Hops" value={hops.length} />
              <Stat label="End-to-end" value={formatLatency(metrics.endToEndMs)} />
              <Stat
                label="Worst hop"
                value={formatWorstHop(metrics)}
                color={metrics.worstHop === null ? undefined : "var(--warning)"}
              />
              <Stat
                label="Loss"
                value={
                  metrics.lossPercent === null
                    ? "—"
                    : `${metrics.lossPercent.toFixed(1)} %`
                }
                color={lossColor}
              />
            </div>
            <p className={styles.insight}>{insight}</p>
          </Card>
        )}

        <div className={styles.content}>
          {showingComparison ? (
            <div className={styles.comparePane}>
              <TracerouteComparison
                a={compareA}
                b={compareB}
                diff={compareDiff}
                onClear={clearCompare}
              />
            </div>
          ) : (
            <div className={styles.livePane}>
              <TracerouteTable rows={hops} />
            </div>
          )}
          <div className={styles.historyPane}>
            <TraceSessionPanel
              sessions={pastTraces}
              disabled={isRunning}
              onOpen={openTrace}
              onDelete={handleDeleteTrace}
              onDeleteMany={handleDeleteTraces}
              compareMode={isCompareSelecting}
              compareSelection={compareSelection}
              onToggleCompare={toggleCompareSelection}
              onStartCompareSelection={startCompareSelection}
              onCompareSelected={compareSelected}
              onCancelCompare={clearCompare}
            />
          </div>
        </div>
      </div>

      <StatusBar>
        {isRunning ? (
          <Live>tracing {target}</Live>
        ) : error.length > 0 ? (
          <span className={styles.statusError}>trace unavailable</span>
        ) : status.length > 0 ? (
          <span>{status}</span>
        ) : showingComparison ? (
          <span>comparing two saved traces</span>
        ) : (
          <span>ready</span>
        )}
        {(isRunning || hops.length > 0 || pastTrace !== null || compareMode) && (
          <span className={styles.progress} data-testid="trace-progress">
            Hop {progressHop}/{MAX_HOPS}
          </span>
        )}
        <span className="vk-statusbar-right">ICMP · max 30 hops · 3 probes/hop</span>
      </StatusBar>
      {confirmDialog}
    </section>
  )
}
