import { BenchmarkResultPanel } from "../components/BenchmarkResultPanel"
import { DownloadSpeedSessionPanel } from "../components/DownloadSpeedSessionPanel"
import { useDownloadSpeedTest } from "../hooks/useDownloadSpeedTest"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import { DEFAULT_HTTP_SETTINGS, HTTP_VERSIONS, type HttpSettings, type HttpVersion } from "../lib/types"

import styles from "./DownloadSpeedView.module.css"

const VERSION_LABELS: Record<HttpVersion, string> = {
  auto: "Auto",
  "http1.1": "HTTP/1.1",
  http2: "HTTP/2 (prior knowledge)",
  http3: "HTTP/3 (not supported)",
}

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
    <div className={styles.benchmarkControls} data-testid="benchmark-controls">
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
        <button
          type="button"
          onClick={onReset}
          className={styles.resetButton}
          disabled={disabled}
          data-testid="benchmark-config-reset"
        >
          Reset benchmark defaults
        </button>
      </div>
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
    <details className={styles.settingsPanel} data-testid="http-settings-panel">
      <summary className={styles.settingsSummary}>HTTP settings</summary>
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
          <button
            type="button"
            onClick={onReset}
            className={styles.resetButton}
            data-testid="http-settings-reset"
          >
            Reset to defaults
          </button>
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
  return (
    <div className={styles.results}>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Average speed</span>
        <span className={styles.resultValue}>{formatMbps(result.averageMbps)}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Total time</span>
        <span className={styles.resultValue}>{formatMs(result.totalTimeMs)}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Time to first byte</span>
        <span className={styles.resultValue}>{formatMs(result.timeToFirstByteMs)}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>DNS resolution</span>
        <span className={styles.resultValue}>{formatMs(result.dnsResolutionMs)}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>TLS handshake</span>
        <span className={styles.resultValue}>
          {result.tlsHandshakeMs === null ? "Not available" : formatMs(result.tlsHandshakeMs)}
        </span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Status code</span>
        <span className={styles.resultValue}>{result.statusCode}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Content size</span>
        <span className={styles.resultValue}>
          {result.contentLength === null ? "Unknown" : formatBytes(result.contentLength)}
        </span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Bytes received</span>
        <span className={styles.resultValue}>{formatBytes(result.bytesReceived)}</span>
      </div>
      <div className={styles.resultCard}>
        <span className={styles.resultLabel}>Final URL</span>
        <span className={styles.resultValue}>{result.finalUrl}</span>
      </div>
    </div>
  )
}

function PageResult({ result }: { readonly result: import("../lib/types").PageSpeedResultDto }) {
  const sortedByDuration = [...result.resources].sort((a, b) => b.durationMs - a.durationMs)
  const slowestByRank = new Map(
    sortedByDuration.slice(0, 5).map((r, index) => [r.url, index + 1]),
  )

  return (
    <>
      <div className={styles.results}>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Resources</span>
          <span className={styles.resultValue}>
            {result.successfulResources}/{result.totalResources}
            {result.failedResources > 0 && (
              <span className={styles.failedCount}> ({result.failedResources} failed)</span>
            )}
          </span>
        </div>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Total time</span>
          <span className={styles.resultValue}>{formatMs(result.totalDurationMs)}</span>
        </div>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Time to first byte</span>
          <span className={styles.resultValue}>{formatMs(result.timeToFirstByteMs)}</span>
        </div>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Average speed</span>
          <span className={styles.resultValue}>{formatMbps(result.averageMbps)}</span>
        </div>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Bytes received</span>
          <span className={styles.resultValue}>{formatBytes(result.totalBytesReceived)}</span>
        </div>
        <div className={styles.resultCard}>
          <span className={styles.resultLabel}>Target URL</span>
          <span className={styles.resultValue}>{result.url}</span>
        </div>
      </div>

      <div>
        <div className={styles.tableCaption}>
          Top 5 slowest resources are highlighted.
        </div>
        <div className={styles.tableWrapper}>
          <table className={styles.resourceTable} data-testid="page-resource-table">
            <thead>
              <tr>
                <th>Type</th>
                <th>URL</th>
                <th>Status</th>
                <th>Size</th>
                <th>Start</th>
                <th>Duration</th>
                <th>Speed</th>
                <th>Error</th>
              </tr>
            </thead>
            <tbody>
              {result.resources.map((resource) => {
                const rank = slowestByRank.get(resource.url)
                const isSlow = rank !== undefined
                return (
                  <tr
                    key={resource.url}
                    className={isSlow ? styles.slowResource : undefined}
                    data-slow={isSlow ? "true" : undefined}
                  >
                    <td>{resource.resourceType}</td>
                    <td title={resource.url} className={styles.urlCell}>
                      {resource.url}
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
      </div>
    </>
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
    <div className={styles.content}>
      <div className={styles.livePane}>
        <div className={styles.view} data-testid="download-speed-view">
          <h1>Web Benchmark</h1>

          <div className={styles.controls}>
            <div className={styles.field}>
              <label htmlFor="download-url">URL</label>
              <input
                id="download-url"
                type="text"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://example.com"
                disabled={isRunning}
                data-testid="download-url"
              />
              {url.trim().length > 0 && !isValid && (
                <span className={styles.inlineError}>
                  URL must start with http:// or https://
                </span>
              )}
            </div>

            <div className={styles.field}>
              <label htmlFor="download-mode">Mode</label>
              <select
                id="download-mode"
                value={mode}
                onChange={(e) => setMode(e.target.value as import("../hooks/useDownloadSpeedTest").SpeedMode)}
                disabled={isRunning}
                data-testid="download-mode"
              >
                <option value="single">Single file</option>
                <option value="page">Full page</option>
                <option value="benchmark">Benchmark (detailed)</option>
              </select>
            </div>

            <div className={styles.actions}>
              <button
                type="button"
                onClick={() => void start()}
                disabled={isRunning || !isValid}
                data-testid="download-start"
              >
                Start
              </button>
              {result !== null && (
                <button type="button" onClick={reset} data-testid="download-reset">
                  Reset
                </button>
              )}
            </div>
          </div>

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

          {error.length > 0 && (
            <div className={`${styles.banner} ${styles.error}`} data-testid="download-error">
              {error}
            </div>
          )}

          {isRunning && progress !== null && (
            <div className={styles.progress}>
              <div className={styles.progressBarTrack}>
                <div
                  className={styles.progressBarFill}
                  style={{ width: `${progressPercent}%` }}
                />
              </div>
              <div className={styles.progressText}>{progressText}</div>
            </div>
          )}

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
        </div>
      </div>
      <div className={styles.historyPane}>
        <DownloadSpeedSessionPanel
          sessions={sessions}
          disabled={isRunning || sessionsLoading}
          onOpen={loadSession}
          onDelete={handleDeleteSession}
        />
      </div>
      {confirmDialog}
    </div>
  )
}
