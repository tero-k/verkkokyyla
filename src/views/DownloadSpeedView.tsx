import { useDownloadSpeedTest } from "../hooks/useDownloadSpeedTest"

import styles from "./DownloadSpeedView.module.css"

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

export default function DownloadSpeedView() {
  const {
    url,
    setUrl,
    isRunning,
    isValid,
    error,
    progress,
    result,
    start,
    reset,
  } = useDownloadSpeedTest()

  const progressPercent =
    progress !== null &&
    progress.contentLength !== null &&
    progress.contentLength > 0
      ? Math.min(100, (progress.bytesReceived / progress.contentLength) * 100)
      : 0

  return (
    <div className={styles.view} data-testid="download-speed-view">
      <h1>Download speed test</h1>

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
          <div className={styles.progressText}>
            {formatBytes(progress.bytesReceived)} downloaded
            {progress.contentLength !== null
              ? ` of ${formatBytes(progress.contentLength)}`
              : ""}
            {" — "}
            {formatMbps(progress.currentMbps)}
          </div>
        </div>
      )}

      {result !== null && (
        <div className={styles.results} data-testid="download-results">
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
            <span className={styles.resultValue}>
              {formatMs(result.timeToFirstByteMs)}
            </span>
          </div>
          <div className={styles.resultCard}>
            <span className={styles.resultLabel}>DNS resolution</span>
            <span className={styles.resultValue}>
              {formatMs(result.dnsResolutionMs)}
            </span>
          </div>
          <div className={styles.resultCard}>
            <span className={styles.resultLabel}>TLS handshake</span>
            <span className={styles.resultValue}>
              {result.tlsHandshakeMs === null
                ? "Not available"
                : formatMs(result.tlsHandshakeMs)}
            </span>
          </div>
          <div className={styles.resultCard}>
            <span className={styles.resultLabel}>Status code</span>
            <span className={styles.resultValue}>{result.statusCode}</span>
          </div>
          <div className={styles.resultCard}>
            <span className={styles.resultLabel}>Content size</span>
            <span className={styles.resultValue}>
              {result.contentLength === null
                ? "Unknown"
                : formatBytes(result.contentLength)}
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
      )}
    </div>
  )
}
