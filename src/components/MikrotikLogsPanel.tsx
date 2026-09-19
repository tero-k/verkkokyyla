import { useEffect, useRef, type ReactNode } from "react"
import { Button, Diamond, Live, Segmented } from "./ui/ui"
import { POLL_SECONDS_OPTIONS, useMikrotikLogs, type LogSeverityFilter } from "../hooks/useMikrotikLogs"

import styles from "./MikrotikLogsPanel.module.css"

const SEVERITY_OPTIONS: readonly { value: LogSeverityFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "warnings", label: "Warnings+" },
  { value: "errors", label: "Errors" },
]

/** Diamond marker color per severity — error/critical share the brick
 * accent, warning the amber one (DESIGN.md §2 status badge pairs). */
const SEVERITY_COLORS: Readonly<Record<string, string>> = {
  critical: "var(--danger)",
  error: "var(--danger)",
  warning: "var(--warning)",
}

/** Wrap every case-insensitive occurrence of `needle` in a <mark> so a
 * free-text search is visible inside the message, not just as a row filter. */
function highlighted(text: string, needle: string): ReactNode {
  const trimmed = needle.trim().toLowerCase()
  if (trimmed === "") return text
  const parts: ReactNode[] = []
  let rest = text
  let key = 0
  while (rest.length > 0) {
    const index = rest.toLowerCase().indexOf(trimmed)
    if (index < 0) {
      parts.push(rest)
      break
    }
    if (index > 0) parts.push(rest.slice(0, index))
    parts.push(
      <mark key={key} className={styles.mark}>
        {rest.slice(index, index + trimmed.length)}
      </mark>,
    )
    key += 1
    rest = rest.slice(index + trimmed.length)
  }
  return parts
}

type MikrotikLogsPanelProps = {
  readonly profileId: number | null
}

export function MikrotikLogsPanel({ profileId }: MikrotikLogsPanelProps) {
  const logs = useMikrotikLogs()
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const pinnedRef = useRef(true)

  // Device switch: the running stream belongs to the PREVIOUS profile —
  // stop it (the hook's stop uses the id captured at start) so the panel
  // never shows one device's logs under another device's name.
  const previousProfileRef = useRef(profileId)
  const stopRef = useRef(logs.stop)
  stopRef.current = logs.stop
  const runningRef = useRef(logs.running)
  runningRef.current = logs.running
  useEffect(() => {
    if (previousProfileRef.current !== profileId) {
      if (runningRef.current) void stopRef.current()
      previousProfileRef.current = profileId
    }
  }, [profileId])

  // Auto-scroll only while the user is parked at the top (newest entries
  // arrive there); scrolling down pauses following so new entries don't
  // yank the view.
  useEffect(() => {
    const node = scrollerRef.current
    if (node !== null && pinnedRef.current) {
      node.scrollTop = 0
    }
  }, [logs.entries])

  function handleScroll(): void {
    const node = scrollerRef.current
    if (node === null) return
    pinnedRef.current = node.scrollTop < 24
  }

  function jumpToLatest(): void {
    const node = scrollerRef.current
    if (node === null) return
    pinnedRef.current = true
    node.scrollTop = 0
  }

  // A new cadence applies immediately: restart the live stream so the
  // router polling picks it up without a manual stop/start cycle.
  function handlePollSecondsChange(value: number): void {
    logs.setPollSeconds(value)
    if (logs.running && profileId !== null) {
      void logs.restart(profileId)
    }
  }

  return (
    <div className={styles.panel} data-testid="mikrotik-logs-panel">
      <section className={styles.section} aria-label="Router log stream">
        <h2>Log stream</h2>

        <div className={styles.toolbar}>
          <div className={styles.controls}>
            <Button
              variant="primary"
              onClick={() => profileId !== null && void logs.start(profileId)}
              disabled={logs.running || profileId === null}
            >
              Connect
            </Button>
            <Button variant="outline-accent" onClick={() => void logs.stop()} disabled={!logs.running}>
              Disconnect
            </Button>
            <Button variant="secondary" onClick={() => logs.clear()} disabled={logs.entries.length === 0 && !logs.running}>
              Clear
            </Button>
            <label className={styles.pollField}>
              Refresh
              <select
                className={styles.pollSelect}
                value={logs.pollSeconds}
                onChange={(event) => handlePollSecondsChange(Number(event.currentTarget.value))}
                aria-label="Refresh frequency"
              >
                {POLL_SECONDS_OPTIONS.map((seconds) => (
                  <option key={seconds} value={seconds}>
                    {seconds} s
                  </option>
                ))}
              </select>
            </label>
            {logs.running && <Live />}
          </div>

          <div className={styles.filters}>
            <Segmented
              options={SEVERITY_OPTIONS}
              value={logs.severityFilter}
              onChange={logs.setSeverityFilter}
              ariaLabel="Severity filter"
            />
            <select
              className={styles.topicSelect}
              value={logs.topicFilter}
              onChange={(event) => logs.setTopicFilter(event.currentTarget.value)}
              aria-label="Topic filter"
              disabled={logs.topics.length === 0}
            >
              <option value="">All topics</option>
              {logs.topics.map((topic) => (
                <option key={topic} value={topic}>
                  {topic}
                </option>
              ))}
            </select>
            <input
              type="search"
              className={styles.search}
              value={logs.textFilter}
              onChange={(event) => logs.setTextFilter(event.currentTarget.value)}
              placeholder="Filter messages and topics"
              aria-label="Filter log messages"
            />
          </div>
        </div>

        {logs.error ? (
          <p className={styles.banner} role="alert">
            {logs.error}
          </p>
        ) : null}

        <div className={styles.scroller} ref={scrollerRef} onScroll={handleScroll}>
          <table className={styles.table} aria-label="Router log entries">
            <thead>
              <tr>
                <th scope="col" className={styles.timeCol}>
                  Time
                </th>
                <th scope="col" className={styles.severityCol}>
                  Severity
                </th>
                <th scope="col" className={styles.topicsCol}>
                  Topics
                </th>
                <th scope="col">
                  Message
                </th>
              </tr>
            </thead>
            <tbody>
              {logs.entries.map((entry) => (
                <tr
                  key={entry.id}
                  data-testid="mikrotik-log-row"
                  data-severity={entry.severity}
                  className={
                    entry.severity === "critical" || entry.severity === "error"
                      ? styles.severityError
                      : entry.severity === "warning"
                        ? styles.severityWarning
                        : undefined
                  }
                >
                  <td className={styles.time}>{entry.time ?? "-"}</td>
                  <td className={styles.severity}>
                    <Diamond color={SEVERITY_COLORS[entry.severity]} small />
                    {entry.severity}
                  </td>
                  <td className={styles.topics}>{highlighted(entry.topics.join(", "), logs.textFilter)}</td>
                  <td className={styles.message}>{highlighted(entry.message, logs.textFilter)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {logs.entries.length === 0 ? (
            <p className={styles.empty} data-testid="mikrotik-logs-empty">
              {logs.running ? "Waiting for log entries…" : "Connect to the stream to see router logs."}
            </p>
          ) : null}
        </div>

        <div className={styles.footer}>
          <span>
            {logs.entries.length === logs.totalCount
              ? `${logs.totalCount} entries`
              : `${logs.entries.length} of ${logs.totalCount} entries (filter active)`}
          </span>
          {!pinnedRef.current && logs.entries.length > 0 ? (
            <button type="button" className={styles.followButton} onClick={jumpToLatest}>
              Jump to latest
            </button>
          ) : null}
        </div>
      </section>
    </div>
  )
}
