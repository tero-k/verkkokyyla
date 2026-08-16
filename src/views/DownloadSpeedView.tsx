import { useDownloadSpeedTest } from "../hooks/useDownloadSpeedTest"
import { DEFAULT_HTTP_SETTINGS, type HttpSettings, type HttpVersion } from "../lib/types"

import styles from "./DownloadSpeedView.module.css"

const VERSION_LABELS: Record<HttpVersion, string> = {
  auto: "Auto",
  "http1.1": "HTTP/1.1",
  http2: "HTTP/2 (prior knowledge)",
}

function clampInt(value: number, min: number, max: number): number {
  return Math.min(Math.max(Math.round(value), min), max)
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
  } = useDownloadSpeedTest()

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
      <h1>Web page speed test</h1>

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
    </div>
  )
}
