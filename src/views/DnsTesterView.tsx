import { useEffect, useMemo, useState } from "react"
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
import styles from "./DnsTesterView.module.css"
import { useConfirmDialog } from "../hooks/useConfirmDialog"

type Tab = "lookup" | "benchmark" | "diagnostics" | "email" | "history"

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

  const latestMetrics: MetricsDto | null = useMemo(() => {
    if (!benchResult) return null
    return benchResult.metrics
  }, [benchResult])

  return (
    <div className={styles.container}>
      <h1>DNS Toolkit</h1>
      <div className={styles.tabs} role="tablist">
        {[
          { key: "lookup", label: "Lookup" },
          { key: "benchmark", label: "Benchmark" },
          { key: "diagnostics", label: "Diagnostics" },
          { key: "email", label: "Email" },
          { key: "history", label: "History" },
        ].map((t) => (
          <button
            key={t.key}
            role="tab"
            aria-selected={tab === (t.key as Tab)}
            className={`${styles.tab}${tab === (t.key as Tab) ? ` ${styles.tabActive}` : ""}`}
            onClick={() => setTab(t.key as Tab)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "lookup" && (
        <section className={styles.panel}>
          <h2>Multi-record lookup</h2>
          <div className={styles.formRow}>
            <label>
              Name
              <input value={lookupName} onChange={(e) => setLookupName(e.target.value)} />
            </label>
          </div>
          <div className={styles.formRow}>
            <label>
              Record types (comma separated)
              <input value={lookupTypes} onChange={(e) => setLookupTypes(e.target.value)} />
            </label>
          </div>
          <EndpointEditor value={lookupEndpoint} onChange={setLookupEndpoint} />
          <button onClick={handleLookup} disabled={lookupRunning} className={styles.primaryButton}>
            {lookupRunning ? "Running..." : "Run lookup"}
          </button>
          {lookupError && <p className={styles.error}>Lookup failed: {lookupError}</p>}
          {lookupSummary && (
            <div className={styles.summary}>
              Completed {lookupSummary.completed}/{lookupSummary.completed + lookupSummary.failed} in{" "}
              {lookupSummary.elapsedMs} ms
            </div>
          )}
          <ul className={styles.eventList}>
            {lookupEvents.map((event, idx) => {
              const result = unwrapResult<QueryResultDto>(event.result)
              return (
                <li key={idx} className={styles.eventItem}>
                  <div>
                    <strong>{event.recordType}</strong>{" "}
                    {result.ok ? (
                      <span>
                        {result.value.rcode} · {result.value.answers.length} answers ·{" "}
                        {result.value.latencyMs} ms ({result.value.transportUsed})
                      </span>
                    ) : (
                      <span className={styles.error}>{result.error.message}</span>
                    )}
                  </div>
                  {result.ok && result.value.answers.length > 0 && (
                    <ul className={styles.answerList}>
                      {result.value.answers.map((answer, answerIdx) => (
                        <li key={answerIdx} className={styles.answerItem}>
                          {answer.data} <span className={styles.ttl}>(ttl {answer.ttl})</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              )
            })}
          </ul>
        </section>
      )}

      {tab === "benchmark" && (
        <section className={styles.panel}>
          <h2>Benchmark</h2>
          <div className={styles.formRow}>
            <label>
              Query name
              <input value={benchName} onChange={(e) => setBenchName(e.target.value)} />
            </label>
          </div>
          <div className={styles.formRow}>
            <label>
              Preset
              <select value={benchPreset} onChange={(e) => setBenchPreset(e.target.value as BenchmarkPreset)}>
                <option value="quick">Quick probe</option>
                <option value="stress">Stress</option>
                <option value="cache-bust">Cache-bust</option>
              </select>
            </label>
          </div>
          <EndpointEditor value={benchEndpoint} onChange={setBenchEndpoint} />
          <button onClick={handleBenchmark} disabled={benchRunning} className={styles.primaryButton}>
            {benchRunning ? "Running..." : "Run benchmark"}
          </button>
          {benchError && <p className={styles.error}>Benchmark failed: {benchError}</p>}
          {latestMetrics && (
            <div className={styles.metrics}>
              <div>Queries: {latestMetrics.count}</div>
              <div>Success: {(latestMetrics.successRate * 100).toFixed(1)}%</div>
              <div>Timeout: {(latestMetrics.timeoutRate * 100).toFixed(1)}%</div>
              <div>Median: {latestMetrics.median?.toFixed(2) ?? "-"} ms</div>
              <div>P95: {latestMetrics.p95?.toFixed(2) ?? latestMetrics.max?.toFixed(2) ?? "-"} ms</div>
              <div>QPS: {latestMetrics.completedQps.toFixed(1)}</div>
            </div>
          )}
          {benchResult && <div className={styles.summary}>Run #{benchResult.runId} · {benchResult.status}</div>}
          {benchCells.length > 0 && (
            <div className={styles.snapshot}>Snapshots: {benchCells.length} · latest {benchCells.at(-1)!.samples.length} samples</div>
          )}
        </section>
      )}

      {tab === "diagnostics" && (
        <section className={styles.panel}>
          <h2>Diagnostics</h2>
          <div className={styles.formRow}>
            <label>
              Domain
              <input value={diagDomain} onChange={(e) => setDiagDomain(e.target.value)} />
            </label>
          </div>
          <EndpointEditor value={diagEndpoint} onChange={setDiagEndpoint} />
          <button onClick={handleDiagnostics} disabled={diagRunning} className={styles.primaryButton}>
            {diagRunning ? "Running..." : "Run diagnostics"}
          </button>
          {diagError && <p className={styles.error}>Diagnostics failed: {diagError}</p>}
          {diagReport && (
            <div className={styles.report}>
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
            </div>
          )}
        </section>
      )}

      {tab === "email" && (
        <section className={styles.panel}>
          <h2>Email security</h2>
          <div className={styles.formRow}>
            <label>
              Domain
              <input value={emailDomain} onChange={(e) => setEmailDomain(e.target.value)} />
            </label>
          </div>
          <div className={styles.formRow}>
            <label>
              DKIM selectors (comma separated)
              <input
                value={emailSelectors}
                onChange={(e) => setEmailSelectors(e.target.value)}
                placeholder="default,google,selector1"
              />
            </label>
          </div>
          <EndpointEditor value={emailEndpoint} onChange={setEmailEndpoint} />
          <button onClick={handleEmail} disabled={emailRunning} className={styles.primaryButton}>
            {emailRunning ? "Running..." : "Check email security"}
          </button>
          {emailError && <p className={styles.error}>Email check failed: {emailError}</p>}
          {emailReport && (
            <div className={styles.report}>
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
            </div>
          )}
        </section>
      )}

      {tab === "history" && (
        <section className={styles.panel}>
          <h2>History</h2>
          {historyLoading ? (
            <p>Loading...</p>
          ) : runs.length === 0 ? (
            <p>No DNS runs recorded yet.</p>
          ) : (
            <ul className={styles.runList}>
              {runs.map((run) => (
                <li key={run.id} className={styles.runItem}>
                  <button className={styles.link} onClick={() => handleLoadRun(run.id)}>
                    #{run.id} {run.kind} · {run.targetInput} · {run.status}
                  </button>
                  <button className={styles.dangerButton} onClick={() => handleDeleteRun(run.id)}>
                    Delete
                  </button>
                </li>
              ))}
            </ul>
          )}
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
        </section>
      )}
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
