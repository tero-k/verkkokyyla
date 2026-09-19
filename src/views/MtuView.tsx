import { useCallback, useEffect, useState } from "react"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "../components/HistoryControls"
import {
  deleteMtuRun,
  listMtuRuns,
  loadMtuRun,
  startMtuProbe,
  stopMtuProbe,
} from "../lib/ipc"
import type {
  MtuMethod,
  MtuProbeDto,
  MtuProbeEvent,
  MtuRunSummaryDto,
  MtuStatusEvent,
  ProbeOutcomeDto,
  ResultKindDto,
  StartMtuDto,
} from "../lib/types"
import {
  Button,
  Card,
  Chip,
  Live,
  SectionHeader,
  Segmented,
  Stat,
  StatusBar,
  ViewHeader,
} from "../components/ui/ui"

import styles from "./MtuView.module.css"

const METHOD_OPTIONS: readonly { readonly value: MtuMethod; readonly label: string }[] = [
  { value: "icmp", label: "ICMP" },
  { value: "tcp", label: "TCP" },
]

const CEILING_OPTIONS = [1500, 9000, 9600, 10240] as const
const DEFAULT_TCP_PORT = 443

type ProbeRow = {
  readonly seq: number
  readonly payloadSize: number
  readonly mtuSize: number
  readonly outcome: ProbeOutcomeDto | null
}

function assertNever(value: never): never {
  throw new Error(`Unhandled MTU value: ${JSON.stringify(value)}`)
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { readonly message: unknown }).message)
  }
  return String(error)
}

function outcomeLabel(outcome: ProbeOutcomeDto | null): string {
  if (outcome === null) return "pending"
  switch (outcome.outcome) {
    case "ok":
      return "ok"
    case "too-big":
      return "too-big"
    case "timeout":
      return "timeout"
    case "error":
      return "error"
    default:
      return assertNever(outcome)
  }
}

function outcomeBadgeClass(outcome: ProbeOutcomeDto | null): string {
  if (outcome === null || outcome.outcome === "timeout") return ""
  switch (outcome.outcome) {
    case "ok":
      return "vk-badge-success"
    case "too-big":
      return "vk-badge-warning"
    case "error":
      return "vk-badge-error"
    default:
      return assertNever(outcome)
  }
}

function outcomeTitle(outcome: ProbeOutcomeDto | null): string | undefined {
  if (outcome?.outcome === "error") return outcome.message
  return undefined
}

function outcomeRtt(outcome: ProbeOutcomeDto | null): string {
  if (outcome?.outcome !== "ok") return "—"
  return outcome.rttMs.toFixed(outcome.rttMs < 10 ? 2 : 1)
}

function outcomeHint(outcome: ProbeOutcomeDto | null): string {
  if (outcome?.outcome !== "too-big" || outcome.hintMtu === null) return "—"
  return String(outcome.hintMtu)
}

function resultBadgeClass(result: ResultKindDto | null): string {
  if (result === null || result.kind === "unreachable") return ""
  switch (result.kind) {
    case "exact":
      return "vk-badge-success"
    case "lower-bound":
      return "vk-badge-warning"
    case "failed":
      return "vk-badge-error"
    default:
      return assertNever(result)
  }
}

function resultLabel(result: ResultKindDto | null): string {
  if (result === null) return "waiting"
  switch (result.kind) {
    case "exact":
      return "exact"
    case "lower-bound":
      return "lower bound"
    case "unreachable":
      return "unreachable"
    case "failed":
      return "failed"
    default:
      return assertNever(result)
  }
}

function resultValue(result: ResultKindDto | null): string {
  if (result === null) return "—"
  switch (result.kind) {
    case "exact":
      return String(result.mtu)
    case "lower-bound":
      return `≥${result.mtu}`
    case "unreachable":
    case "failed":
      return "—"
    default:
      return assertNever(result)
  }
}

function resultDetail(result: ResultKindDto | null): string {
  if (result === null) return "Run a probe to discover the largest path MTU."
  switch (result.kind) {
    case "exact":
      return "The path MTU was confirmed by the probe boundary."
    case "lower-bound":
      switch (result.reason.reason) {
        case "timeout-above":
          return `No Fragmentation Needed replies above ${result.mtu}; ICMP filtering suspected`
        case "ceiling-reached":
          return "Path accepted every probe up to the ceiling"
        default:
          return assertNever(result.reason)
      }
    case "unreachable":
      return "The target did not respond to MTU discovery probes."
    case "failed":
      return result.message
    default:
      return assertNever(result)
  }
}

function historyResult(result: ResultKindDto): string {
  switch (result.kind) {
    case "exact":
      return `${result.mtu} MTU`
    case "lower-bound":
      return `≥${result.mtu} MTU`
    case "unreachable":
      return "Unreachable"
    case "failed":
      return "Failed"
    default:
      return assertNever(result)
  }
}

function formatDuration(durationMs: number | null): string {
  if (durationMs === null) return "—"
  if (durationMs < 1000) return `${durationMs} ms`
  return `${(durationMs / 1000).toFixed(2)} s`
}

function durationFromRun(run: MtuRunSummaryDto): number | null {
  if (run.endedAt === null) return null
  const started = new Date(run.startedAt).getTime()
  const ended = new Date(run.endedAt).getTime()
  if (!Number.isFinite(started) || !Number.isFinite(ended)) return null
  return Math.max(0, ended - started)
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "—"
  return date.toLocaleString(undefined, { hour12: false })
}

function rowsFromLoaded(probes: readonly MtuProbeDto[]): ProbeRow[] {
  return [...probes]
    .sort((left, right) => right.seq - left.seq)
    .map((probe) => ({
      seq: probe.seq,
      payloadSize: probe.payloadSize,
      mtuSize: probe.mtuSize,
      outcome: probe.outcome,
    }))
}

export default function MtuView() {
  const [target, setTarget] = useState("")
  const [method, setMethod] = useState<MtuMethod>("icmp")
  const [ceilingMtu, setCeilingMtu] = useState<(typeof CEILING_OPTIONS)[number]>(1500)
  const [port, setPort] = useState(DEFAULT_TCP_PORT)
  const [isRunning, setIsRunning] = useState(false)
  const [status, setStatus] = useState("Ready")
  const [error, setError] = useState("")
  const [startInfo, setStartInfo] = useState<StartMtuDto | null>(null)
  const [rows, setRows] = useState<ProbeRow[]>([])
  const [result, setResult] = useState<ResultKindDto | null>(null)
  const [probesSent, setProbesSent] = useState(0)
  const [durationMs, setDurationMs] = useState<number | null>(null)
  const [history, setHistory] = useState<MtuRunSummaryDto[]>([])
  const [historyLoading, setHistoryLoading] = useState(true)
  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const runHistory = useSessionHistory(history)

  const refreshHistory = useCallback(async () => {
    setHistoryLoading(true)
    try {
      setHistory(await listMtuRuns())
    } catch (loadError) {
      setError(errorMessage(loadError))
    } finally {
      setHistoryLoading(false)
    }
  }, [])

  useEffect(() => {
    void refreshHistory()
  }, [refreshHistory])

  const handleProbeEvent = useCallback((event: MtuProbeEvent) => {
    switch (event.event) {
      case "attempt":
        setRows((current) => [
          {
            seq: event.seq,
            payloadSize: event.payloadSize,
            mtuSize: event.mtuSize,
            outcome: null,
          },
          ...current.filter((row) => row.seq !== event.seq),
        ])
        return
      case "outcome":
        setRows((current) =>
          current.map((row) =>
            row.seq === event.seq ? { ...row, outcome: event.outcome } : row,
          ),
        )
        return
      default:
        assertNever(event)
    }
  }, [])

  const start = async () => {
    if (target.trim().length === 0 || isRunning) return
    const startedAt = Date.now()
    setIsRunning(true)
    setStatus(`Probing ${target.trim()}`)
    setError("")
    setStartInfo(null)
    setRows([])
    setResult(null)
    setProbesSent(0)
    setDurationMs(null)

    const handleStatusEvent = (event: MtuStatusEvent) => {
      switch (event.event) {
        case "completed":
          setIsRunning(false)
          setResult(event.result)
          setProbesSent(event.probesSent)
          setDurationMs(Date.now() - startedAt)
          setStatus(`Completed run #${event.runId}`)
          void refreshHistory()
          return
        case "cancelled":
          setIsRunning(false)
          setProbesSent(event.probesSent)
          setDurationMs(Date.now() - startedAt)
          setStatus(`Cancelled run #${event.runId} after ${event.probesSent} probes`)
          void refreshHistory()
          return
        case "error":
          setIsRunning(false)
          setError(event.message)
          setStatus("Probe failed")
          return
        default:
          assertNever(event)
      }
    }

    try {
      const started = await startMtuProbe(
        target.trim(),
        method,
        ceilingMtu,
        port,
        handleProbeEvent,
        handleStatusEvent,
      )
      setStartInfo(started)
    } catch (startError) {
      setIsRunning(false)
      setStatus("Probe failed")
      setError(errorMessage(startError))
    }
  }

  const stop = async () => {
    if (!isRunning) return
    try {
      const stopped = await stopMtuProbe()
      setIsRunning(false)
      setProbesSent(stopped.probesSent)
      setStatus(`Cancelled run #${stopped.runId} after ${stopped.probesSent} probes`)
    } catch (stopError) {
      setError(errorMessage(stopError))
    }
  }

  const openRun = async (id: number) => {
    try {
      const loaded = await loadMtuRun(id)
      setTarget(loaded.run.targetInput)
      if (loaded.run.method === "icmp" || loaded.run.method === "tcp") {
        setMethod(loaded.run.method)
      }
      setStartInfo({
        runId: loaded.run.id,
        method: loaded.run.method,
        resolvedIp: loaded.run.resolvedIp,
        answers: [loaded.run.resolvedIp],
      })
      setRows(rowsFromLoaded(loaded.probes))
      setResult(loaded.run.result)
      setProbesSent(loaded.run.probesSent)
      setDurationMs(durationFromRun(loaded.run))
      setStatus(`Loaded run #${loaded.run.id}`)
      setError("")
    } catch (loadError) {
      setError(errorMessage(loadError))
    }
  }

  const deleteRun = async (id: number) => {
    if (!(await confirm("Delete this MTU run?"))) return
    try {
      await deleteMtuRun(id)
      await refreshHistory()
    } catch (deleteError) {
      setError(errorMessage(deleteError))
    }
  }

  const deleteRuns = async (ids: readonly number[]) => {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} MTU runs?`))) return
    try {
      await Promise.all(ids.map((id) => deleteMtuRun(id)))
      await refreshHistory()
    } catch (deleteError) {
      setError(errorMessage(deleteError))
    }
  }

  const handleDeleteSelected = () => {
    const ids = history
      .filter((run) => runHistory.selectedIds.has(run.id))
      .map((run) => run.id)
    void Promise.resolve(deleteRuns(ids)).then(() => runHistory.exitSelectMode())
  }

  const canStart = target.trim().length > 0 && !isRunning

  return (
    <section className={styles.view} data-testid="mtu-view">
      <header className={styles.header}>
        <ViewHeader>
          <h1 className={styles.srOnly}>MTU Discovery</h1>
          <div className={styles.targetChip}>
            <Chip
              label={<label htmlFor="mtu-target">Target</label>}
              value={
                <input
                  id="mtu-target"
                  className={styles.chipInput}
                  type="text"
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                  placeholder="example.com, 192.168.1.1, ::1"
                  disabled={isRunning}
                  data-testid="mtu-target"
                />
              }
              aside={startInfo?.resolvedIp}
            />
          </div>

          <div className={styles.methodChip}>
            <Chip
              label="Method"
              value={
                <fieldset className={styles.segmentedField} disabled={isRunning}>
                  <legend className={styles.srOnly}>Probe method</legend>
                  <Segmented
                    options={METHOD_OPTIONS}
                    value={method}
                    onChange={setMethod}
                    ariaLabel="Probe method"
                  />
                </fieldset>
              }
            />
          </div>

          <div className={styles.optionChip}>
            <Chip
              label={<label htmlFor="mtu-ceiling">Ceiling</label>}
              value={
                <select
                  id="mtu-ceiling"
                  className={styles.chipSelect}
                  value={ceilingMtu}
                  onChange={(event) =>
                    setCeilingMtu(Number(event.target.value) as (typeof CEILING_OPTIONS)[number])
                  }
                  disabled={isRunning}
                  data-testid="mtu-ceiling"
                >
                  {CEILING_OPTIONS.map((option) => (
                    <option key={option} value={option}>
                      {option}
                    </option>
                  ))}
                </select>
              }
            />
          </div>

          {method === "tcp" && (
            <div className={styles.optionChip}>
              <Chip
                label={<label htmlFor="mtu-port">Port</label>}
                value={
                  <input
                    id="mtu-port"
                    className={styles.chipNumber}
                    type="number"
                    min={1}
                    max={65535}
                    value={port}
                    onChange={(event) =>
                      setPort(Math.min(65535, Math.max(1, Number(event.target.value))))
                    }
                    disabled={isRunning}
                    data-testid="mtu-port"
                  />
                }
              />
            </div>
          )}

          <div className={`vk-view-actions ${styles.actions}`}>
            <Button
              variant="outline-danger"
              onClick={() => void stop()}
              disabled={!isRunning}
              data-testid="mtu-stop"
            >
              Stop
            </Button>
            <Button
              variant="primary"
              onClick={() => void start()}
              disabled={!canStart}
              data-testid="mtu-start"
            >
              Start
            </Button>
          </div>
        </ViewHeader>
      </header>

      <div className={`vk-view-body ${styles.body}`}>
        {error.length > 0 && (
          <div className={styles.error} role="alert" data-testid="mtu-error">
            {error}
          </div>
        )}

        <div className={styles.content}>
          <main className={styles.primary}>
            <Card className={styles.resultCard}>
              <SectionHeader
                title="Discovery result"
                aside={startInfo === null ? "AWAITING RUN" : `${startInfo.method.toUpperCase()} · ${startInfo.resolvedIp}`}
              />
              <div className={styles.resultGrid} aria-live="polite" data-testid="mtu-result">
                <div className={styles.mtuResult}>
                  <div className="vk-stat-label">Path MTU</div>
                  <div
                    className={`vk-stat-value vk-stat-value-lg ${styles.mtuValue}`}
                    data-testid="mtu-result-value"
                  >
                    {resultValue(result)}
                  </div>
                  <span className={`vk-badge ${resultBadgeClass(result)}`}>
                    {resultLabel(result)}
                  </span>
                </div>
                <div className={styles.resultStats}>
                  <Stat label="Probes sent" value={probesSent} />
                  <Stat label="Duration" value={formatDuration(durationMs)} />
                </div>
                <p className={styles.resultDetail}>{resultDetail(result)}</p>
              </div>
            </Card>

            <Card className={styles.tableCard}>
              <div className={styles.panelHeader}>
                <SectionHeader
                  title="Probe sequence"
                  aside={`${rows.length} ${rows.length === 1 ? "attempt" : "attempts"}`}
                />
              </div>
              <div className={styles.tableScroll}>
                <table className={styles.probeTable} data-testid="mtu-probe-table">
                  <thead>
                    <tr>
                      <th scope="col">Seq</th>
                      <th scope="col">MTU</th>
                      <th scope="col">Payload</th>
                      <th scope="col">Outcome</th>
                      <th scope="col">RTT ms</th>
                      <th scope="col">Hint MTU</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.length === 0 && (
                      <tr>
                        <td className={styles.emptyTable} colSpan={6}>
                          Enter a target and start discovery to inspect each probe.
                        </td>
                      </tr>
                    )}
                    {rows.map((row) => (
                      <tr key={row.seq} data-testid="mtu-probe-row">
                        <td>{row.seq}</td>
                        <td className={styles.emphasis}>{row.mtuSize}</td>
                        <td>{row.payloadSize}</td>
                        <td>
                          <span
                            className={`vk-badge ${outcomeBadgeClass(row.outcome)}`}
                            title={outcomeTitle(row.outcome)}
                          >
                            {outcomeLabel(row.outcome)}
                          </span>
                        </td>
                        <td>{outcomeRtt(row.outcome)}</td>
                        <td>{outcomeHint(row.outcome)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Card>
          </main>

          <aside className={styles.history} aria-label="Saved MTU runs">
            <Card className={styles.historyCard}>
              <div className={styles.panelHeader}>
                <SectionHeader
                  title="Run history"
                  aside={historyLoading ? "LOADING" : `${history.length} SAVED`}
                />
                {history.length > 0 && !isRunning && !runHistory.selectMode && (
                  <SelectionToggle onClick={runHistory.enterSelectMode} />
                )}
              </div>
              {runHistory.selectMode && (
                <SelectionBar
                  count={runHistory.selectedCount}
                  onDelete={handleDeleteSelected}
                  onCancel={runHistory.exitSelectMode}
                />
              )}
              <div className={styles.tableScroll}>
                <table className={styles.historyTable}>
                  <thead>
                    <tr>
                      {runHistory.selectMode && <th scope="col" aria-hidden="true" />}
                      <th scope="col">Target</th>
                      <th scope="col">Method</th>
                      <th scope="col">Result</th>
                      <th scope="col">Started</th>
                      <th scope="col">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {!historyLoading && history.length === 0 && (
                      <tr>
                        <td className={styles.emptyTable} colSpan={runHistory.selectMode ? 6 : 5}>
                          No saved MTU runs yet.
                        </td>
                      </tr>
                    )}
                    {runHistory.visible.map((run) => (
                      <tr
                        key={run.id}
                        data-testid="mtu-history-row"
                        className={runHistory.selectedIds.has(run.id) ? styles.selectedRow : undefined}
                      >
                        {runHistory.selectMode && (
                          <td>
                            <input
                              className={styles.select}
                              type="checkbox"
                              checked={runHistory.selectedIds.has(run.id)}
                              onChange={() => runHistory.toggleSelected(run.id)}
                              data-testid="mtu-history-select"
                              aria-label={`Select run ${run.id} for deletion`}
                            />
                          </td>
                        )}
                        <td>
                          <span className={styles.historyTarget} title={run.targetInput}>
                            {run.targetInput}
                          </span>
                          <span className={styles.historyIp}>{run.resolvedIp}</span>
                        </td>
                        <td>
                          <span className="vk-badge">{run.method}</span>
                        </td>
                        <td className={styles.emphasis}>{historyResult(run.result)}</td>
                        <td>
                          <time dateTime={run.startedAt}>{formatDateTime(run.startedAt)}</time>
                        </td>
                        <td>
                          <div className={styles.historyActions}>
                            <Button
                              small
                              onClick={() => void openRun(run.id)}
                              disabled={isRunning}
                              data-testid="mtu-history-load"
                            >
                              Load
                            </Button>
                            <Button
                              small
                              variant="outline-danger"
                              onClick={() => void deleteRun(run.id)}
                              disabled={isRunning}
                              data-testid="mtu-history-delete"
                            >
                              Delete
                            </Button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <RevealButton
                totalCount={history.length}
                hiddenCount={runHistory.hiddenCount}
                expanded={runHistory.expanded}
                onToggle={runHistory.toggleExpanded}
              />
            </Card>
          </aside>
        </div>
      </div>

      <StatusBar>
        <span data-testid="mtu-status">
          {isRunning ? <Live>{status}</Live> : status}
        </span>
        <span>{method.toUpperCase()} · ceiling {ceilingMtu}</span>
        <span className="vk-statusbar-right">
          {rows.length > 0 ? `${rows.length} probes shown` : "ready for discovery"}
        </span>
      </StatusBar>
      {confirmDialog}
    </section>
  )
}
