import { useEffect, useMemo, useState } from "react"
import { useSessionHistory } from "../hooks/useSessionHistory"
import {
  RevealButton,
  SelectionBar,
  SelectionToggle,
} from "../components/HistoryControls"
import {
  deleteDnsRun,
  listDnsRuns,
  loadDnsRun,
  runDnsBenchmark,
  runDnsDiagnostics,
  runDnsEmailCheck,
  runDnsLookup,
} from "../lib/ipc"
import type {
  BenchmarkPreset,
  BenchmarkProfile,
  BenchmarkRunDto,
  DiagnosticResultDto,
  DiagnosticStatus,
  DnsDiagnosticsDto,
  DnsProtocol,
  DnsRunSummaryDto,
  DmarcReportDto,
  EmailSecurityReportDto,
  EmailSecurityVerdict,
  LoadedDnsRunDto,
  LookupEventDto,
  LookupSummaryDto,
  MetricsDto,
  QueryResultDto,
  ResolverEndpointDto,
  SampleCell,
  SpfReportDto,
} from "../lib/types"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import {
  Button,
  Card,
  Chip,
  Diamond,
  Meter,
  SectionHeader,
  Segmented,
  StatusBar,
  ViewHeader,
} from "../components/ui/ui"
import styles from "./DnsTesterView.module.css"

type Tab = "lookup" | "benchmark" | "diagnostics" | "email" | "history"

const TAB_OPTIONS: readonly { readonly key: Tab; readonly label: string }[] = [
  { key: "lookup", label: "Lookup" },
  { key: "benchmark", label: "Benchmark" },
  { key: "diagnostics", label: "Diagnostics" },
  { key: "email", label: "Email" },
  { key: "history", label: "History" },
]

const LOOKUP_RECORD_TYPES = ["A", "AAAA", "MX", "NS", "SOA", "TXT"] as const
type LookupRecordType = (typeof LOOKUP_RECORD_TYPES)[number]

const BENCHMARK_PRESET_OPTIONS: readonly {
  readonly value: BenchmarkPreset
  readonly label: string
}[] = [
  { value: "quick", label: "Quick" },
  { value: "stress", label: "Stress" },
  { value: "cache-bust", label: "Cache-bust" },
]

const DEFAULT_ENDPOINT: ResolverEndpointDto = {
  name: "Cloudflare",
  address: "1.1.1.1",
  protocol: "udp",
}

function makeProfile(preset: BenchmarkPreset, queryName: string): BenchmarkProfile {
  const base: BenchmarkProfile = {
    name: "",
    concurrency: 4,
    qpsLimit: 10,
    durationSeconds: 5,
    queryName,
    recordType: "a",
    mix: { uniqueLabels: { base: "mock.test" } },
    cacheBust: false,
  }
  switch (preset) {
    case "quick":
      return { ...base, name: "Quick probe", concurrency: 4, qpsLimit: 10, durationSeconds: 5, cacheBust: true }
    case "stress":
      return { ...base, name: "Stress", concurrency: 32, qpsLimit: 100, durationSeconds: 20, mix: "popularWeighted" }
    case "cache-bust":
      return { ...base, name: "Cache-bust", concurrency: 8, qpsLimit: 50, durationSeconds: 10, cacheBust: true }
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message)
  }
  return String(error)
}

type ResultLike<T> =
  | T
  | { Ok: T }
  | { Err: { kind: string; message: string } }
  | { kind: string; message: string }

function unwrapResult<T>(value: ResultLike<T>): { ok: true; value: T } | { ok: false; error: { kind: string; message: string } } {
  if (typeof value === "object" && value !== null) {
    if ("Ok" in value) return { ok: true, value: (value as { Ok: T }).Ok }
    if ("Err" in value) return { ok: false, error: (value as { Err: { kind: string; message: string } }).Err }
    if ("kind" in value) return { ok: false, error: value as { kind: string; message: string } }
  }
  return { ok: true, value: value as T }
}

export default function DnsTesterView() {
  const [tab, setTab] = useState<Tab>("lookup")

  // Lookup state
  const [lookupName, setLookupName] = useState("example.com")
  const [lookupTypes, setLookupTypes] = useState("A,AAAA,MX,NS,SOA,TXT")
  const [lookupEndpoint, setLookupEndpoint] = useState<ResolverEndpointDto>(DEFAULT_ENDPOINT)
  const [lookupRunning, setLookupRunning] = useState(false)
  const [lookupEvents, setLookupEvents] = useState<LookupEventDto[]>([])
  const [lookupSummary, setLookupSummary] = useState<LookupSummaryDto | null>(null)
  const [lookupError, setLookupError] = useState<string | null>(null)

  // Benchmark state
  const [benchName, setBenchName] = useState("example.com")
  const [benchPreset, setBenchPreset] = useState<BenchmarkPreset>("quick")
  const [benchEndpoint, setBenchEndpoint] = useState<ResolverEndpointDto>(DEFAULT_ENDPOINT)
  const [benchRunning, setBenchRunning] = useState(false)
  const [benchCells, setBenchCells] = useState<SampleCell[]>([])
  const [benchResult, setBenchResult] = useState<BenchmarkRunDto | null>(null)
  const [benchError, setBenchError] = useState<string | null>(null)

  // Diagnostics state
  const [diagDomain, setDiagDomain] = useState("example.com")
  const [diagEndpoint, setDiagEndpoint] = useState<ResolverEndpointDto>(DEFAULT_ENDPOINT)
  const [diagRunning, setDiagRunning] = useState(false)
  const [diagReport, setDiagReport] = useState<DnsDiagnosticsDto | null>(null)
  const [diagError, setDiagError] = useState<string | null>(null)

  // Email security state
  const [emailDomain, setEmailDomain] = useState("example.com")
  const [emailSelectors, setEmailSelectors] = useState("default,google")
  const [emailEndpoint, setEmailEndpoint] = useState<ResolverEndpointDto>(DEFAULT_ENDPOINT)
  const [emailRunning, setEmailRunning] = useState(false)
  const [emailReport, setEmailReport] = useState<EmailSecurityReportDto | null>(null)
  const [emailError, setEmailError] = useState<string | null>(null)

  // History state
  const [runs, setRuns] = useState<DnsRunSummaryDto[]>([])
  const [selectedRun, setSelectedRun] = useState<LoadedDnsRunDto | null>(null)
  const [historyLoading, setHistoryLoading] = useState(false)
  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const runHistory = useSessionHistory(runs)

  useEffect(() => {
    if (tab === "history") {
      refreshHistory()
    }
  }, [tab])

  async function refreshHistory() {
    setHistoryLoading(true)
    try {
      const loaded = await listDnsRuns()
      setRuns(loaded)
    } finally {
      setHistoryLoading(false)
    }
  }

  async function handleLookup() {
    setLookupRunning(true)
    setLookupError(null)
    setLookupEvents([])
    setLookupSummary(null)
    try {
      const types = lookupTypes
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean)
      const summary = await runDnsLookup(lookupName, types, lookupEndpoint, (event) => {
        setLookupEvents((prev) => [...prev, event])
      })
      setLookupSummary(summary)
    } catch (error) {
      setLookupError(errorMessage(error))
    } finally {
      setLookupRunning(false)
    }
  }

  async function handleBenchmark() {
    setBenchRunning(true)
    setBenchError(null)
    setBenchCells([])
    setBenchResult(null)
    try {
      const profile = makeProfile(benchPreset, benchName)
      const result = await runDnsBenchmark(benchEndpoint, profile, (cell) => {
        setBenchCells((prev) => [...prev, cell])
      })
      setBenchResult(result)
    } catch (error) {
      setBenchError(errorMessage(error))
    } finally {
      setBenchRunning(false)
    }
  }

  async function handleDiagnostics() {
    setDiagRunning(true)
    setDiagError(null)
    setDiagReport(null)
    try {
      const report = await runDnsDiagnostics(diagEndpoint, diagDomain)
      setDiagReport(report)
    } catch (error) {
      setDiagError(errorMessage(error))
    } finally {
      setDiagRunning(false)
    }
  }

  async function handleEmail() {
    setEmailRunning(true)
    setEmailError(null)
    setEmailReport(null)
    try {
      const selectors = emailSelectors
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
      const report = await runDnsEmailCheck(emailEndpoint, emailDomain, selectors)
      setEmailReport(report)
    } catch (error) {
      setEmailError(errorMessage(error))
    } finally {
      setEmailRunning(false)
    }
  }

  async function handleLoadRun(id: number) {
    const loaded = await loadDnsRun(id)
    setSelectedRun(loaded)
  }

  async function handleDeleteRun(id: number) {
    if (!(await confirm("Delete this run?"))) return
    await deleteDnsRun(id)
    setSelectedRun(null)
    await refreshHistory()
  }

  async function deleteRuns(ids: readonly number[]) {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} runs?`))) return
    await Promise.all(ids.map((id) => deleteDnsRun(id)))
    setSelectedRun(null)
    await refreshHistory()
  }

  function handleDeleteSelected() {
    const ids = runs
      .filter((run) => runHistory.selectedIds.has(run.id))
      .map((run) => run.id)
    void Promise.resolve(deleteRuns(ids)).then(() => runHistory.exitSelectMode())
  }

  const latestMetrics: MetricsDto | null = useMemo(() => {
    if (!benchResult) return null
    return benchResult.metrics
  }, [benchResult])

  const selectedLookupTypes = useMemo(
    () =>
      new Set(
        lookupTypes
          .split(",")
          .map((recordType) => recordType.trim().toUpperCase())
          .filter(Boolean),
      ),
    [lookupTypes],
  )

  const lookupOutcomes = useMemo(
    () =>
      lookupEvents.map((event, eventIndex) => ({
        event,
        eventIndex,
        result: unwrapResult<QueryResultDto>(event.result),
      })),
    [lookupEvents],
  )

  const successfulLookups = useMemo(
    () =>
      lookupOutcomes.flatMap((outcome) =>
        outcome.result.ok ? [outcome.result.value] : [],
      ),
    [lookupOutcomes],
  )

  const failedLookups = useMemo(
    () =>
      lookupOutcomes.flatMap((outcome) =>
        outcome.result.ok
          ? []
          : [
              {
                event: outcome.event,
                eventIndex: outcome.eventIndex,
                error: outcome.result.error,
              },
            ],
      ),
    [lookupOutcomes],
  )

  const rankedLookupResults = useMemo(
    () => [...successfulLookups].sort((left, right) => left.latencyMs - right.latencyMs),
    [successfulLookups],
  )

  const lookupRecordCount = successfulLookups.reduce(
    (count, result) => count + result.answers.length,
    0,
  )
  const lookupRcodes = new Set(successfulLookups.map((result) => result.rcode))
  const lookupRcode =
    lookupRcodes.size === 1
      ? successfulLookups[0]?.rcode ?? "READY"
      : lookupRcodes.size > 1
        ? "MIXED"
        : "READY"
  const maxLookupLatency = Math.max(
    1,
    ...rankedLookupResults.map((result) => result.latencyMs),
  )
  const validatedResponseCount = successfulLookups.filter((result) => result.adFlag).length
  const ednsResponseCount = successfulLookups.filter((result) => result.ednsPresent).length
  const dnssecValidated =
    successfulLookups.length > 0 && validatedResponseCount === successfulLookups.length
  const latestBenchCell = benchCells.at(-1)

  function toggleLookupRecordType(recordType: LookupRecordType) {
    const currentTypes = lookupTypes
      .split(",")
      .map((currentType) => currentType.trim())
      .filter(Boolean)
    const hasRecordType = currentTypes.some(
      (currentType) => currentType.toUpperCase() === recordType,
    )
    const nextTypes = hasRecordType
      ? currentTypes.filter((currentType) => currentType.toUpperCase() !== recordType)
      : [...currentTypes, recordType]
    setLookupTypes(nextTypes.join(","))
  }

  return (
    <div className={styles.container}>
      <h1 className={styles.srOnly}>DNS Toolkit</h1>

      <div className={styles.headerShell}>
        {tab === "lookup" && (
          <ViewHeader>
            <div className={styles.headerField}>
              <Chip
                label={<label htmlFor="dns-lookup-domain">Domain</label>}
                value={
                  <input
                    id="dns-lookup-domain"
                    aria-label="Domain"
                    className={styles.headerInput}
                    value={lookupName}
                    onChange={(event) => setLookupName(event.target.value)}
                  />
                }
              />
            </div>
            <div
              className={styles.recordTypeWell}
              role="group"
              aria-label="Record types (comma separated)"
            >
              {LOOKUP_RECORD_TYPES.map((recordType) => (
                <button
                  key={recordType}
                  type="button"
                  aria-pressed={selectedLookupTypes.has(recordType)}
                  className={`${styles.recordTypeButton}${
                    selectedLookupTypes.has(recordType) ? ` ${styles.recordTypeButtonActive}` : ""
                  }`}
                  onClick={() => toggleLookupRecordType(recordType)}
                >
                  {recordType}
                </button>
              ))}
            </div>
            <Button variant="primary" onClick={handleLookup} disabled={lookupRunning}>
              {lookupRunning ? "Resolving..." : "Resolve"}
            </Button>
          </ViewHeader>
        )}

        {tab === "benchmark" && (
          <ViewHeader>
            <div className={styles.headerField}>
              <Chip
                label={<label htmlFor="dns-bench-domain">Domain</label>}
                value={
                  <input
                    id="dns-bench-domain"
                    aria-label="Query domain"
                    className={styles.headerInput}
                    value={benchName}
                    onChange={(event) => setBenchName(event.target.value)}
                  />
                }
              />
            </div>
            <Segmented
              options={BENCHMARK_PRESET_OPTIONS}
              value={benchPreset}
              onChange={setBenchPreset}
              ariaLabel="Preset"
            />
            <Button variant="primary" onClick={handleBenchmark} disabled={benchRunning}>
              {benchRunning ? "Running..." : "Run benchmark"}
            </Button>
          </ViewHeader>
        )}

        {tab === "diagnostics" && (
          <ViewHeader>
            <div className={styles.headerField}>
              <Chip
                label={<label htmlFor="dns-diag-domain">Domain</label>}
                value={
                  <input
                    id="dns-diag-domain"
                    aria-label="Domain"
                    className={styles.headerInput}
                    value={diagDomain}
                    onChange={(event) => setDiagDomain(event.target.value)}
                  />
                }
                aside={`${diagEndpoint.name} · ${diagEndpoint.protocol.toUpperCase()}`}
              />
            </div>
            <Button variant="primary" onClick={handleDiagnostics} disabled={diagRunning}>
              {diagRunning ? "Running..." : "Run diagnostics"}
            </Button>
          </ViewHeader>
        )}

        {tab === "email" && (
          <ViewHeader>
            <div className={styles.headerField}>
              <Chip
                label={<label htmlFor="dns-email-domain">Domain</label>}
                value={
                  <input
                    id="dns-email-domain"
                    aria-label="Domain"
                    className={styles.headerInput}
                    value={emailDomain}
                    onChange={(event) => setEmailDomain(event.target.value)}
                  />
                }
              />
            </div>
            <Chip
              label={<label htmlFor="dns-email-selectors">DKIM</label>}
              value={
                <input
                  id="dns-email-selectors"
                  aria-label="DKIM selectors (comma separated)"
                  className={styles.selectorInput}
                  value={emailSelectors}
                  onChange={(event) => setEmailSelectors(event.target.value)}
                  placeholder="default,google,selector1"
                />
              }
            />
            <Button variant="primary" onClick={handleEmail} disabled={emailRunning}>
              {emailRunning ? "Running..." : "Check email security"}
            </Button>
          </ViewHeader>
        )}

        {tab === "history" && (
          <ViewHeader title="DNS run history" subtitle="Persisted benchmarks and diagnostics" />
        )}

        <div className={styles.tabs} role="tablist">
          {TAB_OPTIONS.map((tabOption) => (
          <button
            key={tabOption.key}
            role="tab"
            aria-selected={tab === tabOption.key}
            className={`${styles.tab}${tab === tabOption.key ? ` ${styles.tabActive}` : ""}`}
            onClick={() => setTab(tabOption.key)}
          >
            {tabOption.label}
          </button>
          ))}
        </div>
      </div>

      {tab === "lookup" && (
        <div className={`vk-view-body ${styles.viewBody}`}>
          <details className={styles.settingsPanel}>
            <summary>Query and resolver settings</summary>
            <div className={styles.settingsBody}>
              <div className={styles.formRow}>
                <label>
                  Record types (comma separated)
                  <input
                    value={lookupTypes}
                    onChange={(event) => setLookupTypes(event.target.value)}
                  />
                </label>
              </div>
              <EndpointEditor value={lookupEndpoint} onChange={setLookupEndpoint} />
            </div>
          </details>

          {lookupError && (
            <p className={styles.errorBanner} role="alert">
              Lookup failed: {lookupError}
            </p>
          )}

          <div className={styles.lookupGrid}>
            <div className={styles.answerColumn}>
              <Card className={styles.answerCard}>
                <div className={styles.cardHeader}>
                  <SectionHeader
                    title="Answer section"
                    aside={
                      <span className={lookupSummary ? styles.answerStatus : undefined}>
                        {lookupRunning
                          ? `RESOLVING · ${lookupRecordCount} records`
                          : lookupSummary
                            ? `${lookupRcode} · ${lookupRecordCount} ${lookupRecordCount === 1 ? "record" : "records"} · ${lookupSummary.elapsedMs} ms`
                            : `READY · ${selectedLookupTypes.size} types`}
                      </span>
                    }
                  />
                </div>

                <div className={styles.tableScroll}>
                  <table className={styles.recordsTable}>
                    <thead>
                      <tr>
                        <th>Name</th>
                        <th>Type</th>
                        <th>TTL</th>
                        <th>Data</th>
                      </tr>
                    </thead>
                    <tbody>
                      {lookupOutcomes.length === 0 && (
                        <tr>
                          <td colSpan={4} className={styles.emptyTable}>
                            Resolve a name to inspect returned records.
                          </td>
                        </tr>
                      )}
                      {lookupOutcomes.flatMap(({ event, eventIndex, result }) => {
                        if (!result.ok) {
                          return [
                            <tr key={`error-${eventIndex}`}>
                              <td title={event.queryName}>{event.queryName}</td>
                              <td>
                                <span
                                  className={styles.recordType}
                                  data-record-type={event.recordType.toUpperCase()}
                                >
                                  {event.recordType}
                                </span>
                              </td>
                              <td className={styles.ttl}>-</td>
                              <td className={styles.tableError}>{result.error.message}</td>
                            </tr>,
                          ]
                        }

                        if (result.value.answers.length === 0) {
                          return [
                            <tr key={`empty-${eventIndex}`}>
                              <td title={result.value.queryName}>{result.value.queryName}</td>
                              <td>
                                <span
                                  className={styles.recordType}
                                  data-record-type={result.value.recordType.toUpperCase()}
                                >
                                  {result.value.recordType}
                                </span>
                              </td>
                              <td className={styles.ttl}>-</td>
                              <td className={styles.noData}>{result.value.rcode} · no answers</td>
                            </tr>,
                          ]
                        }

                        return result.value.answers.map((answer, answerIndex) => (
                          <tr key={`${eventIndex}-${answerIndex}`}>
                            <td title={result.value.queryName}>{result.value.queryName}</td>
                            <td>
                              <span
                                className={styles.recordType}
                                data-record-type={result.value.recordType.toUpperCase()}
                              >
                                {result.value.recordType}
                              </span>
                            </td>
                            <td className={styles.ttl}>{answer.ttl}</td>
                            <td title={answer.data} className={styles.recordData}>
                              {answer.data}
                            </td>
                          </tr>
                        ))
                      })}
                    </tbody>
                  </table>
                </div>
              </Card>

              <Card className={styles.dnssecStrip}>
                <div className={styles.dnssecState}>
                  <Diamond
                    color={
                      dnssecValidated
                        ? "var(--accent-bright)"
                        : successfulLookups.length > 0
                          ? "var(--warning)"
                          : "var(--text-faint)"
                    }
                  />
                  <span>
                    {dnssecValidated
                      ? "DNSSEC validated"
                      : successfulLookups.length > 0
                        ? "DNSSEC not validated"
                        : "DNSSEC status pending"}
                  </span>
                </div>
                <span className={styles.dnssecDetail}>
                  {successfulLookups.length > 0
                    ? `${validatedResponseCount}/${successfulLookups.length} AD · ${ednsResponseCount}/${successfulLookups.length} EDNS0`
                    : "Available after a successful response"}
                </span>
              </Card>
            </div>

            <Card className={styles.raceCard} pad>
              <SectionHeader
                title="Resolver race"
                aside={`${rankedLookupResults.length} timed ${rankedLookupResults.length === 1 ? "query" : "queries"}`}
              />
              <div className={styles.raceList}>
                {rankedLookupResults.length === 0 && (
                  <p className={styles.emptyState}>
                    Timing lanes appear as record-type queries return.
                  </p>
                )}
                {rankedLookupResults.map((result, rankIndex) => {
                  const meterColor =
                    rankIndex === 0
                      ? "var(--accent-bright)"
                      : rankIndex === rankedLookupResults.length - 1
                        ? "var(--danger)"
                        : "var(--warning)"
                  return (
                    <div className={styles.raceItem} key={`${result.recordType}-${rankIndex}`}>
                      <Meter
                        label={
                          <span className={styles.raceLabel}>
                            <span className={styles.resolverName}>{lookupEndpoint.name}</span>
                            <span className={styles.resolverAddress}>{lookupEndpoint.address}</span>
                            <span className={styles.raceType}>{result.recordType}</span>
                          </span>
                        }
                        value={`${result.latencyMs} ms`}
                        pct={(result.latencyMs / maxLookupLatency) * 100}
                        color={meterColor}
                      />
                      <div className={styles.raceNote}>
                        {result.queryName} · {result.answers.length} {result.answers.length === 1 ? "answer" : "answers"} · {result.transportUsed.toUpperCase()}
                      </div>
                    </div>
                  )
                })}
                {failedLookups.map(({ event, eventIndex, error }) => (
                  <div className={styles.raceFailure} key={`race-error-${eventIndex}`}>
                    <span>{event.recordType} · {lookupEndpoint.address}</span>
                    <span>{error.message}</span>
                  </div>
                ))}
              </div>
              {lookupSummary && (
                <div className={styles.raceFooter}>
                  <span>Query completion</span>
                  <p>
                    {lookupSummary.completed} of {lookupSummary.completed + lookupSummary.failed} record types completed through {lookupSummary.resolver}.
                  </p>
                </div>
              )}
            </Card>
          </div>
        </div>
      )}

      {tab === "benchmark" && (
        <div className={`vk-view-body ${styles.viewBody}`}>
          <details className={styles.settingsPanel}>
            <summary>Resolver settings</summary>
            <div className={styles.settingsBody}>
              <EndpointEditor value={benchEndpoint} onChange={setBenchEndpoint} />
            </div>
          </details>
          <Card className={styles.panel} pad>
            <h2 className={styles.srOnly}>Benchmark</h2>
            <SectionHeader
              title="Benchmark result"
              aside={benchResult ? `RUN #${benchResult.runId} · ${benchResult.status}` : "AWAITING RUN"}
            />
            {benchError && <p className={styles.errorBanner}>Benchmark failed: {benchError}</p>}
            {latestMetrics ? (
              <div className={styles.metrics}>
                <div><span>Queries</span><strong>{latestMetrics.count}</strong></div>
                <div><span>Success</span><strong>{(latestMetrics.successRate * 100).toFixed(1)}%</strong></div>
                <div><span>Timeout</span><strong>{(latestMetrics.timeoutRate * 100).toFixed(1)}%</strong></div>
                <div><span>Median</span><strong>{latestMetrics.median?.toFixed(2) ?? "-"} ms</strong></div>
                <div><span>P95</span><strong>{latestMetrics.p95?.toFixed(2) ?? latestMetrics.max?.toFixed(2) ?? "-"} ms</strong></div>
                <div><span>QPS</span><strong>{latestMetrics.completedQps.toFixed(1)}</strong></div>
              </div>
            ) : (
              <p className={styles.emptyState}>Run a benchmark to inspect resolver throughput and latency.</p>
            )}
            {latestBenchCell && (
              <div className={styles.snapshot}>
                Snapshots: {benchCells.length} · latest {latestBenchCell.samples.length} samples
              </div>
            )}
          </Card>
        </div>
      )}

      {tab === "diagnostics" && (
        <div className={`vk-view-body ${styles.viewBody}`}>
          <details className={styles.settingsPanel}>
            <summary>Resolver settings</summary>
            <div className={styles.settingsBody}>
              <EndpointEditor value={diagEndpoint} onChange={setDiagEndpoint} />
            </div>
          </details>
          {diagError && <p className={styles.errorBanner}>Diagnostics failed: {diagError}</p>}
          {diagReport ? (
            <Card className={styles.report} pad>
              <SectionHeader
                title="Diagnostics report"
                aside={`${diagReport.durationMs} ms`}
              />
              <div className={styles.emailSectionHeader}>
                <h3>{diagReport.domain}</h3>
                <DiagnosticChip status={diagReport.overallStatus} />
              </div>
              <p>{diagReport.summary}</p>
              <p className={styles.ttl}>Duration: {diagReport.durationMs} ms</p>

              {diagReport.results.map((r) => (
                <DiagnosticResult key={r.id} result={r} />
              ))}

              {diagReport.inventory.length > 0 && (
                <div className={styles.section}>
                  <h4>Record inventory</h4>
                  <ul className={styles.answerList}>
                    {diagReport.inventory.map((item) => (
                      <li key={item.recordType} className={styles.answerItem}>
                        {item.recordType}: {item.status}
                        {item.count !== undefined && ` (${item.count})`}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {Boolean(diagReport.technicalDetails) && (
                <details>
                  <summary>Technical details</summary>
                  <pre className={styles.pre}>
                    {String(JSON.stringify(diagReport.technicalDetails, null, 2))}
                  </pre>
                </details>
              )}
            </Card>
          ) : (
            <Card className={styles.panel} pad>
              <h2 className={styles.srOnly}>Diagnostics</h2>
              <SectionHeader title="Diagnostics report" aside="AWAITING RUN" />
              <p className={styles.emptyState}>Run diagnostics to inspect DNS health and delegation evidence.</p>
            </Card>
          )}
        </div>
      )}

      {tab === "email" && (
        <div className={`vk-view-body ${styles.viewBody}`}>
          <details className={styles.settingsPanel}>
            <summary>Email and resolver settings</summary>
            <div className={styles.settingsBody}>
              <div className={styles.formRow}>
                <label>
                  DKIM selectors (comma separated)
                  <input
                    value={emailSelectors}
                    onChange={(event) => setEmailSelectors(event.target.value)}
                    placeholder="default,google,selector1"
                  />
                </label>
              </div>
              <EndpointEditor value={emailEndpoint} onChange={setEmailEndpoint} />
            </div>
          </details>
          {emailError && <p className={styles.errorBanner}>Email check failed: {emailError}</p>}
          {emailReport ? (
            <Card className={styles.report} pad>
              <SectionHeader title="Email security report" aside={`${emailReport.elapsedMs} ms`} />
              <h3>{emailReport.domain}</h3>
              <p>Elapsed: {emailReport.elapsedMs} ms</p>
              <EmailSection title="SPF" report={emailReport.spf}>
                <div>All mechanism: {emailReport.spf.allMechanism ?? "-"}</div>
                <div>
                  Lookups: {emailReport.spf.lookupCount}{" "}
                  {emailReport.spf.lookupLimitOk ? "(within limit)" : "(over limit)"}
                </div>
                {emailReport.spf.record && (
                  <pre className={styles.pre}>{emailReport.spf.record}</pre>
                )}
              </EmailSection>
              <EmailSection title="DMARC" report={emailReport.dmarc}>
                <div>Policy: {emailReport.dmarc.policy ?? "-"}</div>
                <div>Subdomain policy: {emailReport.dmarc.subdomainPolicy ?? "-"}</div>
                <div>Pct: {emailReport.dmarc.pct ?? "-"}</div>
                <div>Reporting: {emailReport.dmarc.reportingAddress ?? "-"}</div>
                {emailReport.dmarc.record && (
                  <pre className={styles.pre}>{emailReport.dmarc.record}</pre>
                )}
              </EmailSection>
              <h4>DKIM</h4>
              {emailReport.dkim.length === 0 ? (
                <p>No selectors checked.</p>
              ) : (
                emailReport.dkim.map((sel) => (
                  <div key={sel.selector} className={styles.emailSubSection}>
                    <div className={styles.emailSubHeader}>
                      <strong>{sel.selector}</strong>
                      <VerdictChip verdict={sel.verdict} />
                    </div>
                    <div>Found: {sel.found ? "yes" : "no"}</div>
                    <div>Key present: {sel.keyPresent ? "yes" : "no"}</div>
                    {sel.keyBitsApprox !== null && (
                      <div>Approx key size: {sel.keyBitsApprox} bits</div>
                    )}
                    <div>Revoked: {sel.revoked ? "yes" : "no"}</div>
                    {sel.record && <pre className={styles.pre}>{sel.record}</pre>}
                    {sel.notes.length > 0 && <NoteList notes={sel.notes} />}
                  </div>
                  ))
                )}
            </Card>
          ) : (
            <Card className={styles.panel} pad>
              <h2 className={styles.srOnly}>Email security</h2>
              <SectionHeader title="Email security report" aside="AWAITING RUN" />
              <p className={styles.emptyState}>Check a domain to inspect SPF, DKIM, and DMARC posture.</p>
            </Card>
          )}
        </div>
      )}

      {tab === "history" && (
        <div className={`vk-view-body ${styles.viewBody}`}>
          <Card className={styles.panel} pad>
          <h2 className={styles.srOnly}>History</h2>
          <div className={styles.historyHeader}>
            <SectionHeader title="Saved runs" aside={`${runs.length} ${runs.length === 1 ? "RUN" : "RUNS"}`} />
            {runs.length > 0 && !runHistory.selectMode && (
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
          {historyLoading ? (
            <p className={styles.emptyState}>Loading...</p>
          ) : runs.length === 0 ? (
            <p className={styles.emptyState}>No DNS runs recorded yet.</p>
          ) : (
            <ul className={styles.runList}>
              {runHistory.visible.map((run) => (
                <li
                  key={run.id}
                  className={`${styles.runItem}${runHistory.selectedIds.has(run.id) ? ` ${styles.selected}` : ""}`}
                >
                  {runHistory.selectMode && (
                    <input
                      className={styles.select}
                      type="checkbox"
                      checked={runHistory.selectedIds.has(run.id)}
                      onChange={() => runHistory.toggleSelected(run.id)}
                      data-testid="dns-run-select"
                      aria-label={`Select run ${run.id} for deletion`}
                    />
                  )}
                  <Button small className={styles.link} onClick={() => handleLoadRun(run.id)}>
                    #{run.id} {run.kind} · {run.targetInput} · {run.status}
                  </Button>
                  <Button variant="outline-danger" small onClick={() => handleDeleteRun(run.id)}>
                    Delete
                  </Button>
                </li>
              ))}
            </ul>
          )}
          <RevealButton
            totalCount={runs.length}
            hiddenCount={runHistory.hiddenCount}
            expanded={runHistory.expanded}
            onToggle={runHistory.toggleExpanded}
          />
          {selectedRun && (
            <div className={styles.report}>
              <h3>Run #{selectedRun.id}</h3>
              <pre className={styles.pre}>{selectedRun.configJson}</pre>
              {selectedRun.targets.map((t) => (
                <details key={t.target}>
                  <summary>{t.target}</summary>
                  <pre className={styles.pre}>{t.metricsJson}</pre>
                </details>
              ))}
            </div>
          )}
          </Card>
        </div>
      )}

      <StatusBar>
        {tab === "lookup" && (
          <>
            <span>{lookupEndpoint.protocol.toUpperCase()} · {lookupEndpoint.address}</span>
            {ednsResponseCount > 0 && <span>EDNS0</span>}
            {successfulLookups.length > 0 && successfulLookups.every((result) => !result.truncated) && (
              <span>no truncation</span>
            )}
            <span className="vk-statusbar-right">
              {lookupSummary
                ? `${lookupSummary.completed}/${lookupSummary.completed + lookupSummary.failed} queries`
                : "ready"}
            </span>
          </>
        )}
        {tab === "benchmark" && (
          <>
            <span>{benchEndpoint.protocol.toUpperCase()} · {benchEndpoint.address}</span>
            <span className="vk-statusbar-right">{benchResult ? benchResult.status : "ready"}</span>
          </>
        )}
        {tab === "diagnostics" && (
          <>
            <span>{diagEndpoint.protocol.toUpperCase()} · {diagEndpoint.address}</span>
            <span className="vk-statusbar-right">{diagReport ? diagReport.overallStatus : "ready"}</span>
          </>
        )}
        {tab === "email" && (
          <>
            <span>{emailEndpoint.protocol.toUpperCase()} · {emailEndpoint.address}</span>
            <span className="vk-statusbar-right">{emailReport ? `${emailReport.elapsedMs} ms` : "ready"}</span>
          </>
        )}
        {tab === "history" && (
          <>
            <span>DNS history</span>
            <span className="vk-statusbar-right">{runs.length} saved</span>
          </>
        )}
      </StatusBar>
      {confirmDialog}
    </div>
  )
}

function EndpointEditor({
  value,
  onChange,
}: {
  value: ResolverEndpointDto
  onChange: (value: ResolverEndpointDto) => void
}) {
  return (
    <div className={styles.endpoint}>
      <label>
        Name
        <input
          value={value.name}
          onChange={(e) => onChange({ ...value, name: e.target.value })}
        />
      </label>
      <label>
        Address
        <input
          value={value.address}
          onChange={(e) => onChange({ ...value, address: e.target.value })}
        />
      </label>
      <label>
        Protocol
        <select
          value={value.protocol}
          onChange={(e) => onChange({ ...value, protocol: e.target.value as DnsProtocol })}
        >
          <option value="udp">UDP</option>
          <option value="tcp">TCP</option>
          <option value="tls">TLS</option>
          <option value="https">HTTPS</option>
        </select>
      </label>
    </div>
  )
}

function VerdictChip({ verdict }: { verdict: EmailSecurityVerdict }) {
  const cls =
    verdict === "pass"
      ? styles.verdictPass
      : verdict === "warn"
        ? styles.verdictWarn
        : styles.verdictFail
  return <span className={`${styles.verdict} ${cls}`}>{verdict.toUpperCase()}</span>
}

function DiagnosticChip({ status }: { status: DiagnosticStatus }) {
  const cls =
    status === "pass"
      ? styles.verdictPass
      : status === "info"
        ? styles.statusInfo
        : status === "inconclusive"
          ? styles.statusInconclusive
          : status === "warning"
            ? styles.verdictWarn
            : styles.verdictFail
  return <span className={`${styles.verdict} ${cls}`}>{status === "inconclusive" ? "NOT VERIFIED" : status.toUpperCase()}</span>
}

function DiagnosticResult({ result }: { result: DiagnosticResultDto }) {
  return (
    <div className={styles.emailSubSection}>
      <div className={styles.emailSubHeader}>
        <strong>{result.title}</strong>
        <DiagnosticChip status={result.status} />
      </div>
      <p>{result.summary}</p>
      {result.impact && (
        <p>
          <strong>Impact:</strong> {result.impact}
        </p>
      )}
      {result.recommendation && (
        <p>
          <strong>Recommendation:</strong> {result.recommendation}
        </p>
      )}
      {result.evidence.length > 0 && (
        <ul className={styles.answerList}>
          {result.evidence.map((e, i) => (
            <li key={i} className={styles.answerItem}>
              <strong>{e.label}:</strong> {e.value}
            </li>
          ))}
        </ul>
      )}
      {Boolean(result.technicalDetails) && (
        <details>
          <summary>Technical details</summary>
          <pre className={styles.pre}>{String(JSON.stringify(result.technicalDetails, null, 2))}</pre>
        </details>
      )}
    </div>
  )
}

function NoteList({ notes }: { notes: readonly string[] }) {
  return (
    <ul className={styles.noteList}>
      {notes.map((note, idx) => (
        <li key={idx}>{note}</li>
      ))}
    </ul>
  )
}

function EmailSection({
  title,
  report,
  children,
}: {
  title: string
  report: SpfReportDto | DmarcReportDto
  children: React.ReactNode
}) {
  return (
    <div className={styles.emailSection}>
      <div className={styles.emailSectionHeader}>
        <h4>{title}</h4>
        <VerdictChip verdict={report.verdict} />
      </div>
      {children}
      {report.notes.length > 0 && <NoteList notes={report.notes} />}
    </div>
  )
}
