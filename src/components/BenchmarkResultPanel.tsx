import type {
  WebBenchmarkResult,
  WebBenchmarkProtocolSummary,
  WebBenchmarkRun,
} from "../lib/types"
import styles from "../views/DownloadSpeedView.module.css"

function formatMs(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return "—"
  return `${ms.toFixed(2)} ms`
}

function formatBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes) || bytes === 0) return "—"
  const units = ["B", "KB", "MB", "GB"]
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 2)} ${units[i]}`
}

function formatThroughput(bps: number | null): string {
  if (bps === null || !Number.isFinite(bps) || bps === 0) return "—"
  const mbps = (bps * 8) / 1_000_000
  return `${mbps.toFixed(2)} Mbps`
}

function formatCount(count: number): string {
  return count.toString()
}

function MetricCard({ label, value }: { readonly label: string; readonly value: string }) {
  return (
    <div className={styles.resultCard}>
      <span className={styles.resultLabel}>{label}</span>
      <span className={styles.resultValue}>{value}</span>
    </div>
  )
}

function SummaryTable({ summaries }: { readonly summaries: readonly WebBenchmarkProtocolSummary[] }) {
  return (
    <div className={styles.tableWrapper}>
      <table className={styles.resourceTable} data-testid="benchmark-summary-table">
        <thead>
          <tr>
            <th>Protocol</th>
            <th>Runs</th>
            <th>Failed</th>
            <th>Total avg</th>
            <th>Total p50</th>
            <th>Total p95</th>
            <th>Total p99</th>
            <th>TTFB avg</th>
            <th>Download avg</th>
            <th>Size avg</th>
            <th>Throughput avg</th>
          </tr>
        </thead>
        <tbody>
          {summaries.map((summary) => (
            <tr key={summary.requestedProtocol}>
              <td>{summary.negotiatedProtocol ?? summary.requestedProtocol}</td>
              <td>{formatCount(summary.successfulRuns)}</td>
              <td>{formatCount(summary.failedRuns)}</td>
              <td>{formatMs(summary.totalMs.average)}</td>
              <td>{formatMs(summary.totalMs.p50)}</td>
              <td>{formatMs(summary.totalMs.p95)}</td>
              <td>{formatMs(summary.totalMs.p99)}</td>
              <td>{formatMs(summary.ttfbMs.average)}</td>
              <td>{formatMs(summary.downloadMs.average)}</td>
              <td>{formatBytes(summary.responseBytes.average)}</td>
              <td>{formatThroughput(summary.throughputBytesPerSecond.average)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function ComparisonTable({
  summaries,
}: {
  readonly summaries: readonly WebBenchmarkProtocolSummary[]
}) {
  const comparable = summaries.filter((s) => s.successfulRuns >= 3 && s.totalMs.count > 0)
  if (comparable.length < 2) {
    return (
      <p className={styles.fieldHint}>
        Select at least two protocols with 3+ successful runs each to see comparisons.
      </p>
    )
  }

  const rows: { readonly key: string; readonly text: string }[] = []
  for (let i = 0; i < comparable.length; i++) {
    for (let j = i + 1; j < comparable.length; j++) {
      const a = comparable[i]
      const b = comparable[j]
      if (b.totalMs.p50 <= 0) continue
      const diff = a.totalMs.p50 - b.totalMs.p50
      const pct = (diff / b.totalMs.p50) * 100
      const aProto = a.negotiatedProtocol ?? a.requestedProtocol
      const bProto = b.negotiatedProtocol ?? b.requestedProtocol
      const text =
        Math.abs(pct) < 0.05
          ? `${aProto} and ${bProto} are within noise (<0.1%) for median total latency`
          : `${pct > 0 ? aProto : bProto} is ${Math.abs(pct).toFixed(1)}% slower than ${
              pct > 0 ? bProto : aProto
            } for median total latency`
      rows.push({
        key: `${a.requestedProtocol}-${b.requestedProtocol}-p50`,
        text,
      })
    }
  }

  return (
    <div className={styles.tableWrapper}>
      <table className={styles.resourceTable} data-testid="benchmark-comparison-table">
        <thead>
          <tr>
            <th>Comparison</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <td>{row.text}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function ConcurrencyBlock({ result }: { readonly result: WebBenchmarkResult }) {
  if (!result.concurrency) return null
  const c = result.concurrency
  return (
    <div className={styles.results} data-testid="benchmark-concurrency">
      <MetricCard label="Concurrency" value={c.concurrency.toString()} />
      <MetricCard
        label="Total requests"
        value={`${c.successfulRequests}/${c.totalRequests}`}
      />
      <MetricCard label="Failed requests" value={c.failedRequests.toString()} />
      <MetricCard label="Requests/sec" value={c.requestsPerSecond.toFixed(2)} />
      <MetricCard label="Avg latency" value={formatMs(c.averageLatencyMs)} />
      <MetricCard label="p50 latency" value={formatMs(c.p50LatencyMs)} />
      <MetricCard label="p95 latency" value={formatMs(c.p95LatencyMs)} />
      <MetricCard label="p99 latency" value={formatMs(c.p99LatencyMs)} />
      <MetricCard label="Total bytes" value={formatBytes(c.totalBytes)} />
      <MetricCard
        label="Aggregate throughput"
        value={formatThroughput(c.aggregateThroughputBytesPerSecond)}
      />
      <MetricCard
        label="Negotiated protocol"
        value={c.negotiatedProtocol ?? "—"}
      />
    </div>
  )
}

function RunsTable({ runs }: { readonly runs: readonly WebBenchmarkRun[] }) {
  return (
    <details className={styles.settingsPanel}>
      <summary className={styles.settingsSummary}>
        Raw runs ({runs.length})
      </summary>
      <div className={styles.tableWrapper}>
        <table className={styles.resourceTable} data-testid="benchmark-runs-table">
          <thead>
            <tr>
              <th>Run</th>
              <th>Requested</th>
              <th>Negotiated</th>
              <th>Total</th>
              <th>TTFB</th>
              <th>DNS</th>
              <th>Status</th>
              <th>Mode</th>
              <th>Warmup</th>
              <th>Error</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => (
              <tr key={run.runId}>
                <td>{run.runId}</td>
                <td>{run.requestedProtocol}</td>
                <td>{run.negotiatedProtocol ?? "—"}</td>
                <td>{formatMs(run.totalMs)}</td>
                <td>{formatMs(run.ttfbMs)}</td>
                <td>{formatMs(run.dnsMs)}</td>
                <td>{run.statusCode ?? "—"}</td>
                <td>{run.connectionMode}</td>
                <td>{run.isWarmup ? "yes" : "no"}</td>
                <td>{run.errorType ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  )
}

export function BenchmarkResultPanel({ result }: { readonly result: WebBenchmarkResult }) {
  const firstSummary = result.summaries[0]
  const firstProbe = firstSummary?.probe

  return (
    <div data-testid="benchmark-result-panel">
      <h2>Benchmark results</h2>

      <div className={styles.results}>
        <MetricCard label="Total" value={formatMs(firstSummary?.totalMs.average ?? null)} />
        <MetricCard label="TTFB" value={formatMs(firstSummary?.ttfbMs.average ?? null)} />
        <MetricCard label="DNS" value={formatMs(firstSummary?.dnsMs.average ?? null)} />
        <MetricCard label="Connect" value={formatMs(firstSummary?.connectMs.average ?? null)} />
        <MetricCard label="TLS" value={formatMs(firstSummary?.tlsMs.average ?? null)} />
        <MetricCard label="Download" value={formatMs(firstSummary?.downloadMs.average ?? null)} />
        <MetricCard
          label="HTTP version"
          value={firstSummary?.negotiatedProtocol ?? firstSummary?.requestedProtocol ?? "—"}
        />
        <MetricCard
          label="Response size"
          value={formatBytes(firstSummary?.responseBytes.average ?? null)}
        />
        <MetricCard
          label="Throughput"
          value={formatThroughput(firstSummary?.throughputBytesPerSecond.average ?? null)}
        />
      </div>

      {firstProbe && (
        <>
          <h3>Connection probe</h3>
          <div className={styles.results}>
            <MetricCard label="Probe DNS" value={formatMs(firstProbe.dnsMs)} />
            <MetricCard label="Probe connect" value={formatMs(firstProbe.connectMs)} />
            <MetricCard label="Probe TLS" value={formatMs(firstProbe.tlsMs)} />
            <MetricCard label="TLS version" value={firstProbe.tlsVersion ?? "—"} />
            <MetricCard label="ALPN" value={firstProbe.alpn ?? "—"} />
            <MetricCard label="Remote IP" value={firstProbe.remoteIp ?? "—"} />
            <MetricCard label="IP version" value={firstProbe.ipVersion ?? "—"} />
          </div>
          {firstProbe.error && (
            <div className={`${styles.banner} ${styles.error}`}>Probe: {firstProbe.error}</div>
          )}
        </>
      )}

      <h3>Per-protocol summary</h3>
      <SummaryTable summaries={result.summaries} />

      <h3>Protocol comparison</h3>
      <ComparisonTable summaries={result.summaries} />

      {result.concurrency && (
        <>
          <h3>Concurrency test</h3>
          <ConcurrencyBlock result={result} />
        </>
      )}

      <h3>Raw measurements</h3>
      <RunsTable runs={result.runs} />
    </div>
  )
}
