import { useMemo, useState } from "react"
import { BenchmarkResultPanel } from "../components/BenchmarkResultPanel"
import { DownloadSpeedSessionPanel } from "../components/DownloadSpeedSessionPanel"
import {
  useDownloadSpeedTest,
  type SpeedMode,
} from "../hooks/useDownloadSpeedTest"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import {
  applyFiltersAndSort,
  splitUrlForDisplay,
  type ResourceFilters,
  type SortDir,
  type SortKey,
  type StatusFilter,
} from "../lib/pageResources"
import {
  DEFAULT_HTTP_SETTINGS,
  HTTP_VERSIONS,
  PAGE_RESOURCE_TYPES,
  type HttpSettings,
  type HttpVersion,
  type PageResourceType,
} from "../lib/types"
import {
  Button,
  Card,
  Chip,
  Live,
  Meter,
  SectionHeader,
  Stat,
  StatusBar,
  ViewHeader,
} from "../components/ui/ui"

import styles from "./DownloadSpeedView.module.css"

const VERSION_LABELS: Record<HttpVersion, string> = {
  auto: "Auto",
  "http1.1": "HTTP/1.1",
  http2: "HTTP/2 (prior knowledge)",
  http3: "HTTP/3 (not supported)",
}

const SPEED_MODES: readonly { readonly value: SpeedMode; readonly label: string }[] = [
  { value: "single", label: "Download" },
  { value: "page", label: "Page load" },
  { value: "benchmark", label: "Throughput" },
]

const RESOURCE_SORT_COLUMNS = [
  { key: "type", label: "Type", defaultDir: "asc" },
  { key: "url", label: "URL", defaultDir: "asc" },
  { key: "status", label: "Status", defaultDir: "desc" },
  { key: "size", label: "Size", defaultDir: "desc" },
  { key: "start", label: "Start", defaultDir: "asc" },
  { key: "duration", label: "Duration", defaultDir: "desc" },
  { key: "speed", label: "Speed", defaultDir: "desc" },
  { key: "error", label: "Error", defaultDir: "asc" },
] satisfies readonly {
  readonly key: SortKey
  readonly label: string
  readonly defaultDir: SortDir
}[]

function clampInt(value: number, min: number, max: number): number {
  return Math.min(Math.max(Math.round(value), min), max)
}

type BenchmarkConfig = ReturnType<typeof useDownloadSpeedTest>["benchmarkConfig"]

type BenchmarkConfigUpdate = ReturnType<typeof useDownloadSpeedTest>["updateBenchmarkConfig"]

function BenchmarkControls({
  config,
  onChange,
  onReset,
  disabled,
}: {
  readonly config: BenchmarkConfig
  readonly onChange: BenchmarkConfigUpdate
  readonly onReset: () => void
  readonly disabled: boolean
}) {
  const toggleProtocol = (version: HttpVersion) => {
    const current = config.protocols
    const next = current.includes(version)
      ? current.filter((v) => v !== version)
      : [...current, version]
    onChange({ protocols: next.length > 0 ? next : ["auto"] })
  }

  return (
    <div data-testid="benchmark-controls">
      <Card className={styles.benchmarkControls} pad>
        <SectionHeader
          title="Benchmark options"
          aside={`${config.runs} runs · ${config.connectionMode} connection`}
        />
        <div className={styles.benchmarkGrid}>
          <div className={styles.field}>
            <span className={styles.fieldLabel}>Protocols</span>
            <div className={styles.checkboxGroup}>
              {HTTP_VERSIONS.map((version) => (
                <label key={version} className={styles.checkboxLabel}>
                  <input
                    type="checkbox"
                    checked={config.protocols.includes(version)}
                    onChange={() => toggleProtocol(version)}
                    disabled={disabled}
                  />
                  {VERSION_LABELS[version]}
                </label>
              ))}
            </div>
          </div>

          <div className={styles.field}>
            <label htmlFor="benchmark-runs">Runs</label>
            <input
              id="benchmark-runs"
              type="number"
              min={1}
              max={100}
              value={config.runs}
              onChange={(e) =>
                onChange({ runs: clampInt(Number(e.target.value), 1, 100) })
              }
              disabled={disabled}
              data-testid="benchmark-runs"
            />
          </div>

          <div className={styles.field}>
            <span className={styles.fieldLabel}>Connection mode</span>
            <div className={styles.radioGroup}>
              {[
                { value: "cold", label: "Cold" },
                { value: "warm", label: "Warm" },
              ].map((option) => (
                <label key={option.value} className={styles.radioLabel}>
                  <input
                    type="radio"
                    name="benchmark-connection-mode"
                    value={option.value}
                    checked={config.connectionMode === option.value}
                    onChange={() =>
                      onChange({ connectionMode: option.value as import("../lib/types").ConnectionMode })
                    }
                    disabled={disabled}
                  />
                  {option.label}
                </label>
              ))}
            </div>
          </div>

          <div className={styles.field}>
            <label htmlFor="benchmark-concurrency">Concurrency</label>
            <select
              id="benchmark-concurrency"
              value={config.concurrency ?? ""}
              onChange={(e) => {
                const value = e.target.value
                onChange({ concurrency: value === "" ? null : Number(value) })
              }}
              disabled={disabled}
              data-testid="benchmark-concurrency"
            >
              <option value="">Off</option>
              {[1, 5, 10, 25, 50].map((level) => (
                <option key={level} value={level}>
                  {level}
                </option>
              ))}
            </select>
          </div>

          <div className={`${styles.field} ${styles.checkboxField}`}>
            <label htmlFor="benchmark-probe">
              <input
                id="benchmark-probe"
                type="checkbox"
                checked={config.probe}
                onChange={(e) => onChange({ probe: e.target.checked })}
                disabled={disabled}
                data-testid="benchmark-probe"
              />
              Probe connection (DNS/TCP/TLS)
            </label>
          </div>

          <div className={styles.settingsActions}>
            <Button
              small
              onClick={onReset}
              disabled={disabled}
              data-testid="benchmark-config-reset"
            >
              Reset benchmark defaults
            </Button>
          </div>
        </div>
      </Card>
    </div>
  )
}

function HttpSettingsPanel({
  settings,
  onChange,
  onReset,
}: {
  readonly settings: HttpSettings
  readonly onChange: (patch: Partial<HttpSettings>) => void
  readonly onReset: () => void
}) {
  return (
    <details className={`vk-card ${styles.settingsPanel}`} data-testid="http-settings-panel">
      <summary className={styles.settingsSummary}>
        <span>HTTP settings</span>
        <span className={styles.settingsMeta}>
          {VERSION_LABELS[settings.version]} · {settings.ipFamily.toUpperCase()}
        </span>
      </summary>
      <div className={styles.settingsGrid}>
        <div className={styles.field}>
          <label htmlFor="http-version">HTTP version</label>
          <select
            id="http-version"
            value={settings.version}
            onChange={(e) =>
              onChange({ version: e.target.value as HttpVersion })
            }
            data-testid="http-version"
          >
            <option value="auto">{VERSION_LABELS.auto}</option>
            <option value="http1.1">{VERSION_LABELS["http1.1"]}</option>
            <option value="http2">{VERSION_LABELS.http2}</option>
          </select>
          <span className={styles.fieldHint}>
            HTTP/2 uses prior knowledge and only works with compatible servers.
          </span>
        </div>

        <div className={styles.field}>
          <label htmlFor="http-connect-timeout">Connect timeout (seconds)</label>
          <input
            id="http-connect-timeout"
            type="number"
            min={1}
            max={300}
            value={settings.connectTimeoutSec}
            onChange={(e) =>
              onChange({
                connectTimeoutSec: clampInt(Number(e.target.value), 1, 300),
              })
            }
            data-testid="http-connect-timeout"
          />
        </div>

        <div className={styles.field}>
          <label htmlFor="http-request-timeout">Request timeout (seconds)</label>
          <input
            id="http-request-timeout"
            type="number"
            min={1}
            max={300}
            value={settings.requestTimeoutSec}
            onChange={(e) =>
              onChange({
                requestTimeoutSec: clampInt(Number(e.target.value), 1, 300),
              })
            }
            data-testid="http-request-timeout"
          />
        </div>

        <div className={`${styles.field} ${styles.checkboxField}`}>
          <label htmlFor="http-follow-redirects">
            <input
              id="http-follow-redirects"
              type="checkbox"
              checked={settings.followRedirects}
              onChange={(e) =>
                onChange({ followRedirects: e.target.checked })
              }
              data-testid="http-follow-redirects"
            />
            Follow redirects
          </label>
        </div>

        <div className={styles.field}>
          <label htmlFor="http-max-redirects">Max redirects</label>
          <input
            id="http-max-redirects"
            type="number"
            min={0}
            max={100}
            disabled={!settings.followRedirects}
            value={settings.maxRedirects}
            onChange={(e) =>
              onChange({
                maxRedirects: clampInt(Number(e.target.value), 0, 100),
              })
            }
            data-testid="http-max-redirects"
          />
        </div>

        <div className={`${styles.field} ${styles.checkboxField}`}>
          <label htmlFor="http-compression">
            <input
              id="http-compression"
              type="checkbox"
              checked={settings.compression}
              onChange={(e) =>
                onChange({ compression: e.target.checked })
              }
              data-testid="http-compression"
            />
            Enable gzip/brotli/deflate compression
          </label>
        </div>

        <div className={styles.field}>
          <label htmlFor="http-ip-family">IP family</label>
          <select
            id="http-ip-family"
            value={settings.ipFamily}
            onChange={(e) =>
              onChange({ ipFamily: e.target.value as import("../lib/types").IpFamily })
            }
            data-testid="http-ip-family"
          >
            <option value="auto">Auto</option>
            <option value="ipv4">IPv4</option>
            <option value="ipv6">IPv6</option>
          </select>
        </div>

        <div className={styles.field}>
          <label htmlFor="http-read-timeout">Read timeout (seconds, 0 = disabled)</label>
          <input
            id="http-read-timeout"
            type="number"
            min={0}
            max={300}
            value={settings.readTimeoutSec}
            onChange={(e) =>
              onChange({
                readTimeoutSec: clampInt(Number(e.target.value), 0, 300),
              })
            }
            data-testid="http-read-timeout"
          />
        </div>

        <div className={styles.field}>
          <label htmlFor="http-user-agent">User-Agent</label>
          <input
            id="http-user-agent"
            type="text"
            value={settings.userAgent}
            onChange={(e) => onChange({ userAgent: e.target.value })}
            placeholder={`Default: ${DEFAULT_HTTP_SETTINGS.userAgent || "built-in"}`}
            data-testid="http-user-agent"
          />
        </div>

        <div className={styles.settingsActions}>
          <Button small onClick={onReset} data-testid="http-settings-reset">
            Reset to defaults
          </Button>
        </div>
      </div>
    </details>
  )
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B"
  const units = ["B", "KB", "MB", "GB"]
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  )
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 2)} ${units[i]}`
}

function formatMbps(mbps: number | null): string {
  if (mbps === null || !Number.isFinite(mbps)) return "-"
  return `${mbps.toFixed(2)} Mbps`
}

function formatMs(ms: number | null): string {
  if (ms === null) return "-"
  return `${ms} ms`
}

function SingleResult({ result }: { readonly result: import("../lib/types").DownloadSpeedResultDto }) {
  const totalTime = Math.max(result.totalTimeMs, 1)

  return (
    <Card className={styles.singleResult} pad>
      <SectionHeader
        title="Download result"
        aside={`HTTP ${result.statusCode} · ${formatBytes(result.bytesReceived)}`}
      />
      <div className={styles.singleResultGrid}>
        <div className={styles.primaryStat}>
          <Stat label="Average speed" value={formatMbps(result.averageMbps)} large />
        </div>
        <div className={styles.results}>
          <div className={styles.statCell}>
            <Stat label="Total time" value={formatMs(result.totalTimeMs)} />
          </div>
          <div className={styles.statCell}>
            <Stat label="Status code" value={result.statusCode} />
          </div>
          <div className={styles.statCell}>
            <Stat
              label="Content size"
              value={result.contentLength === null ? "Unknown" : formatBytes(result.contentLength)}
            />
          </div>
          <div className={styles.statCell}>
            <Stat label="Bytes received" value={formatBytes(result.bytesReceived)} />
          </div>
        </div>
        <div className={styles.meterStack}>
          <Meter
            label="Time to first byte"
            value={formatMs(result.timeToFirstByteMs)}
            pct={((result.timeToFirstByteMs ?? 0) / totalTime) * 100}
            color="var(--warning)"
          />
          <Meter
            label="DNS resolution"
            value={formatMs(result.dnsResolutionMs)}
            pct={((result.dnsResolutionMs ?? 0) / totalTime) * 100}
            color="var(--graph-rtt)"
          />
          <Meter
            label="TLS handshake"
            value={result.tlsHandshakeMs === null ? "Not available" : formatMs(result.tlsHandshakeMs)}
            pct={((result.tlsHandshakeMs ?? 0) / totalTime) * 100}
            color="var(--graph-rtt)"
          />
        </div>
      </div>
      <div className={styles.finalUrl}>
        <span>Final URL</span>
        <strong title={result.finalUrl}>{result.finalUrl}</strong>
      </div>
    </Card>
  )
}

function PageResult({ result }: { readonly result: import("../lib/types").PageSpeedResultDto }) {
  const [sortKey, setSortKey] = useState<SortKey>("start")
  const [sortDir, setSortDir] = useState<SortDir>("asc")
  const [filters, setFilters] = useState<ResourceFilters>(() => ({
    types: new Set(PAGE_RESOURCE_TYPES),
    status: "all",
    slowestOnly: false,
  }))
  // Keyed by object identity: distinct resources may share a URL (e.g. the
  // document and a self-referencing <link>), and URL-keyed React rows corrupt
  // on re-sort when keys collide.
  const slowestByRank = useMemo(
    () =>
      new Map(
        [...result.resources]
          .sort((a, b) => b.durationMs - a.durationMs)
          .slice(0, 5)
          .map((resource, index) => [resource, index + 1] as const),
      ),
    [result.resources],
  )
  const visibleResources = useMemo(
    () => applyFiltersAndSort(result.resources, filters, sortKey, sortDir),
    [filters, result.resources, sortDir, sortKey],
  )

  const toggleType = (resourceType: PageResourceType) => {
    setFilters((previous) => {
      const types = new Set(previous.types)
      if (types.has(resourceType)) {
        types.delete(resourceType)
      } else {
        types.add(resourceType)
      }
      return { ...previous, types }
    })
  }

  const setStatusFilter = (value: string) => {
    let status: StatusFilter
    switch (value) {
      case "all":
      case "2xx":
      case "3xx":
      case "4xx":
      case "5xx":
      case "failed":
        status = value
        break
      default:
        return
    }
    setFilters((previous) => ({ ...previous, status }))
  }

  const changeSort = (key: SortKey, defaultDir: SortDir) => {
    if (sortKey === key) {
      setSortDir((previous) => (previous === "asc" ? "desc" : "asc"))
      return
    }
    setSortKey(key)
    setSortDir(defaultDir)
  }

  return (
    <div className={styles.pageResult}>
      <Card className={styles.pageSummary} pad>
        <SectionHeader
          title="Page load result"
          aside={`${result.successfulResources}/${result.totalResources} resources`}
        />
        <div className={styles.results}>
          <div className={styles.statCell}>
            <Stat
              label="Resources"
              value={
                <>
                  {result.successfulResources}/{result.totalResources}
                  {result.failedResources > 0 && (
                    <span className={styles.failedCount}> ({result.failedResources} failed)</span>
                  )}
                </>
              }
            />
          </div>
          <div className={styles.statCell}>
            <Stat label="Total time" value={formatMs(result.totalDurationMs)} />
          </div>
          <div className={styles.statCell}>
            <Stat label="Time to first byte" value={formatMs(result.timeToFirstByteMs)} />
          </div>
          <div className={styles.statCell}>
            <Stat label="Average speed" value={formatMbps(result.averageMbps)} />
          </div>
          <div className={styles.statCell}>
            <Stat label="Bytes received" value={formatBytes(result.totalBytesReceived)} />
          </div>
          <div className={styles.statCell}>
            <Stat label="Target URL" value={result.url} />
          </div>
        </div>
      </Card>

      <Card className={styles.tableCard}>
        <div className={styles.panelHeading}>
          <SectionHeader
            title="Request waterfall"
            aside={`${result.totalResources} requests · ${formatBytes(result.totalBytesReceived)}`}
          />
          <div className={styles.tableCaption} aria-live="polite">
            Showing {visibleResources.length} of {result.totalResources} resources · Top 5 slowest
            resources are highlighted.
          </div>
        </div>
        <div className={styles.filterBar} aria-label="Resource filters">
          <div className={styles.filterGroup}>
            <span className={styles.filterLabel}>Type</span>
            <div className={styles.typeChips}>
              {PAGE_RESOURCE_TYPES.map((resourceType) => (
                <button
                  key={resourceType}
                  type="button"
                  className={`${styles.typeChip}${
                    filters.types.has(resourceType) ? ` ${styles.typeChipActive}` : ""
                  }`}
                  aria-pressed={filters.types.has(resourceType)}
                  onClick={() => toggleType(resourceType)}
                >
                  {resourceType}
                </button>
              ))}
            </div>
          </div>
          <label className={styles.statusFilter}>
            <span className={styles.filterLabel}>Status</span>
            <select
              value={filters.status}
              onChange={(event) => setStatusFilter(event.target.value)}
            >
              <option value="all">All</option>
              <option value="2xx">2xx</option>
              <option value="3xx">3xx</option>
              <option value="4xx">4xx</option>
              <option value="5xx">5xx</option>
              <option value="failed">Failed</option>
            </select>
          </label>
          <button
            type="button"
            className={`${styles.typeChip} ${styles.slowestToggle}${
              filters.slowestOnly ? ` ${styles.typeChipActive}` : ""
            }`}
            aria-pressed={filters.slowestOnly}
            onClick={() =>
              setFilters((previous) => ({
                ...previous,
                slowestOnly: !previous.slowestOnly,
              }))
            }
          >
            Slowest only
          </button>
        </div>
        <div className={styles.tableWrapper}>
          <table className={styles.resourceTable} data-testid="page-resource-table">
            <thead>
              <tr>
                {RESOURCE_SORT_COLUMNS.map((column) => {
                  const active = sortKey === column.key
                  return (
                    <th
                      key={column.key}
                      scope="col"
                      className={styles.sortHeader}
                      aria-sort={active ? (sortDir === "asc" ? "ascending" : "descending") : "none"}
                    >
                      <button
                        type="button"
                        onClick={() => changeSort(column.key, column.defaultDir)}
                        aria-label={`Sort by ${column.label}`}
                      >
                        <span>{column.label}</span>
                        <span className={styles.sortIndicator} aria-hidden="true">
                          {active ? (sortDir === "asc" ? "↑" : "↓") : "↕"}
                        </span>
                      </button>
                    </th>
                  )
                })}
              </tr>
            </thead>
            <tbody>
              {visibleResources.map((resource, index) => {
                const rank = slowestByRank.get(resource)
                const isSlow = rank !== undefined
                return (
                  <tr
                    key={`${index}:${resource.url}`}
                    className={isSlow ? styles.slowResource : undefined}
                    data-slow={isSlow ? "true" : undefined}
                  >
                    <td>{resource.resourceType}</td>
                    <td title={resource.url} className={styles.urlCell}>
                      {(() => {
                        const { head, tail } = splitUrlForDisplay(resource.url)
                        return (
                          <span className={styles.urlText}>
                            <span className={styles.urlHead}>{head}</span>
                            {tail !== "" && <span className={styles.urlTail}>{tail}</span>}
                          </span>
                        )
                      })()}
                      {isSlow && (
                        <span className={styles.slowBadge} aria-label={`Slowest resource rank ${rank}`}>
                          #{rank} slowest
                        </span>
                      )}
                    </td>
                    <td>{resource.statusCode ?? "-"}</td>
                    <td>{formatBytes(resource.bytesReceived)}</td>
                    <td>{resource.startOffsetMs} ms</td>
                    <td>{resource.durationMs} ms</td>
                    <td>{formatMbps(resource.averageMbps)}</td>
                    <td>{resource.error ?? "-"}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  )
}

function EmptyThroughputPanel() {
  return (
    <Card className={styles.emptyChartCard} pad>
      <SectionHeader title="Throughput" aside="waiting for sample" />
      <div className={styles.emptyChart}>
        <svg
          viewBox="0 0 640 180"
          preserveAspectRatio="none"
          role="img"
          aria-label="Empty throughput chart"
        >
          <title>Throughput appears here after a test</title>
          <line x1="0" y1="24" x2="640" y2="24" />
          <line x1="0" y1="68" x2="640" y2="68" />
          <line x1="0" y1="112" x2="640" y2="112" />
          <line x1="0" y1="156" x2="640" y2="156" />
        </svg>
        <p>Run a download, page load, or detailed benchmark to plot measured throughput.</p>
      </div>
      <div className={styles.chartAxis}>
        <span>0 s</span>
        <span>measurement window</span>
        <span>complete</span>
      </div>
      <div className={styles.legend} aria-label="Throughput chart legend">
        <span className={styles.legendChip}>
          <span className={`${styles.legendSwatch} ${styles.legendThroughput}`} />
          throughput
        </span>
        <span className={styles.legendChip}>
          <span className={`${styles.legendSwatch} ${styles.legendAverage}`} />
          session average
        </span>
      </div>
    </Card>
  )
}

export default function DownloadSpeedView() {
  const {
    url,
    setUrl,
    mode,
    setMode,
    benchmarkConfig,
    updateBenchmarkConfig,
    resetBenchmarkConfig,
    isRunning,
    isValid,
    error,
    progress,
    result,
    start,
    reset,
    httpSettings,
    updateHttpSettings,
    resetHttpSettings,
    sessions,
    sessionsLoading,
    loadSession,
    deleteSession,
  } = useDownloadSpeedTest()

  const { confirm, dialog: confirmDialog } = useConfirmDialog()
  const handleDeleteSession = async (id: number) => {
    if (await confirm("Delete this speed test?")) {
      await deleteSession(id)
    }
  }
  const handleDeleteSessions = async (ids: readonly number[]) => {
    if (ids.length === 0) return
    if (!(await confirm(`Delete ${ids.length} speed tests?`))) return
    await Promise.all(ids.map((id) => deleteSession(id)))
  }

  const progressPercent =
    progress?.kind === "single" &&
    progress.event.contentLength !== null &&
    progress.event.contentLength > 0
      ? Math.min(
          100,
          (progress.event.bytesReceived / progress.event.contentLength) * 100,
        )
      : progress?.kind === "page" && progress.event.total > 0
        ? Math.min(100, (progress.event.completed / progress.event.total) * 100)
        : 0

  const progressText =
    progress?.kind === "single" ? (
      <>
        {formatBytes(progress.event.bytesReceived)} downloaded
        {progress.event.contentLength !== null
          ? ` of ${formatBytes(progress.event.contentLength)}`
          : ""}
        {" — "}
        {formatMbps(progress.event.currentMbps)}
      </>
    ) : progress?.kind === "page" ? (
      <>
        {progress.event.completed}/{progress.event.total} resources
        {progress.event.resource.resourceType !== "document" && (
          <> — {progress.event.resource.resourceType}</>
        )}
      </>
    ) : null

  return (
    <div className={styles.view} data-testid="download-speed-view">
      <h1 className={styles.srOnly}>Web Benchmark</h1>
      <header className={styles.header}>
        <ViewHeader>
          <div className={styles.headerStack}>
            <div className={styles.toolbar}>
              <div className={styles.urlControl}>
                <div className={styles.urlChip}>
                  <Chip
                    label={<label htmlFor="download-url">URL</label>}
                    value={
                      <input
                        id="download-url"
                        className={styles.urlInput}
                        type="text"
                        value={url}
                        onChange={(event) => setUrl(event.target.value)}
                        placeholder="https://example.com"
                        disabled={isRunning}
                        data-testid="download-url"
                      />
                    }
                    aside={`${VERSION_LABELS[httpSettings.version]} · ${httpSettings.ipFamily.toUpperCase()}`}
                  />
                </div>
                {url.trim().length > 0 && !isValid && (
                  <span className={styles.inlineError}>
                    URL must start with http:// or https://
                  </span>
                )}
              </div>

              <div className={styles.modeChip}>
                <Chip
                  label={<label htmlFor="download-mode">Mode</label>}
                  value={
                    <select
                      id="download-mode"
                      value={mode}
                      onChange={(event) => setMode(event.target.value as SpeedMode)}
                      disabled={isRunning}
                      data-testid="download-mode"
                    >
                      <option value="single">Single file</option>
                      <option value="page">Full page</option>
                      <option value="benchmark">Benchmark (detailed)</option>
                    </select>
                  }
                />
              </div>

              <div className={`vk-view-actions ${styles.headerActions}`}>
                {result !== null && (
                  <Button onClick={reset} data-testid="download-reset">
                    Reset
                  </Button>
                )}
                <Button
                  variant="primary"
                  onClick={() => void start()}
                  disabled={isRunning || !isValid}
                  data-testid="download-start"
                >
                  {isRunning ? "Running…" : mode === "benchmark" ? "Benchmark" : "Start"}
                </Button>
              </div>
            </div>

            <nav className={styles.tabs} role="tablist" aria-label="Web benchmark mode">
              {SPEED_MODES.map((item) => (
                <button
                  key={item.value}
                  type="button"
                  role="tab"
                  aria-selected={mode === item.value}
                  aria-controls="download-benchmark-body"
                  className={`${styles.tab}${mode === item.value ? ` ${styles.tabActive}` : ""}`}
                  disabled={isRunning}
                  onClick={() => setMode(item.value)}
                >
                  {item.label}
                </button>
              ))}
            </nav>
          </div>
        </ViewHeader>
      </header>

      <div id="download-benchmark-body" className={styles.body}>
        <div className={styles.content}>
          <section className={styles.livePane} aria-label="Web benchmark results">
            <div className={styles.configPanels}>
              {mode === "benchmark" && (
                <BenchmarkControls
                  config={benchmarkConfig}
                  onChange={updateBenchmarkConfig}
                  onReset={resetBenchmarkConfig}
                  disabled={isRunning}
                />
              )}

              <HttpSettingsPanel
                settings={httpSettings}
                onChange={updateHttpSettings}
                onReset={resetHttpSettings}
              />
            </div>

            {error.length > 0 && (
              <div
                className={`${styles.banner} ${styles.error}`}
                role="alert"
                data-testid="download-error"
              >
                {error}
              </div>
            )}

            {isRunning && progress !== null && (
              <Card className={styles.progress} pad>
                <SectionHeader title="Transfer progress" aside={<Live>measuring</Live>} />
                <Meter
                  label={progressText}
                  value={`${Math.round(progressPercent)}%`}
                  pct={progressPercent}
                />
              </Card>
            )}

            {result === null && <EmptyThroughputPanel />}
            {result?.kind === "single" && (
              <div data-testid="download-results">
                <SingleResult result={result.data} />
              </div>
            )}
            {result?.kind === "page" && (
              <div data-testid="download-results">
                <PageResult result={result.data} />
              </div>
            )}
            {result?.kind === "benchmark" && (
              <div data-testid="download-results">
                <BenchmarkResultPanel result={result.data} />
              </div>
            )}
          </section>
          <aside className={styles.historyPane} aria-label="Saved speed tests">
            <DownloadSpeedSessionPanel
              sessions={sessions}
              disabled={isRunning || sessionsLoading}
              onOpen={loadSession}
              onDelete={handleDeleteSession}
              onDeleteMany={handleDeleteSessions}
            />
          </aside>
        </div>
      </div>
      <StatusBar>
        {isRunning ? <Live>benchmark active</Live> : <span className="vk-statusbar-ok">ready</span>}
        <span>{SPEED_MODES.find((item) => item.value === mode)?.label}</span>
        {result !== null && <span>latest result shown</span>}
        <span className="vk-statusbar-right">
          {sessionsLoading ? "loading history" : `${sessions.length} saved sessions`}
        </span>
      </StatusBar>
      {confirmDialog}
    </div>
  )
}
