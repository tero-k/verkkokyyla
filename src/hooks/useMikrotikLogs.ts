import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { mikrotikLogStart, mikrotikLogStop } from "../lib/ipc"
import type { MikrotikLogEntry, MikrotikLogEvent, MikrotikLogStatusEvent } from "../lib/types"

/** Visible buffer cap, mirroring the ping live table (README "Notes"). */
export const LOG_BUFFER_CAP = 500

/** Refresh cadence choices, in seconds (the backend clamps to 1–60). */
export const POLL_SECONDS_OPTIONS: readonly number[] = [1, 2, 5, 10, 30]

export const DEFAULT_POLL_SECONDS = 2

export type LogSeverityFilter = "all" | "warnings" | "errors"

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  if (typeof err === "object" && err !== null && "message" in err) {
    const message: unknown = err.message
    if (typeof message === "string") return message
  }
  return String(err)
}

function isAtLeastWarning(entry: MikrotikLogEntry): boolean {
  return entry.severity === "critical" || entry.severity === "error" || entry.severity === "warning"
}

function isAtLeastError(entry: MikrotikLogEntry): boolean {
  return entry.severity === "critical" || entry.severity === "error"
}

/**
 * Client-side filter over the capped buffer. Pure and exported for tests.
 * Severity levels are inclusive: "warnings" keeps warning + error +
 * critical, "errors" keeps error + critical.
 */
export function filterLogEntries(
  entries: readonly MikrotikLogEntry[],
  severity: LogSeverityFilter,
  text: string,
  topic: string,
): MikrotikLogEntry[] {
  const needle = text.trim().toLowerCase()
  return entries.filter((entry) => {
    if (severity === "warnings" && !isAtLeastWarning(entry)) return false
    if (severity === "errors" && !isAtLeastError(entry)) return false
    if (topic !== "" && !entry.topics.includes(topic)) return false
    if (
      needle !== "" &&
      !entry.message.toLowerCase().includes(needle) &&
      !entry.topics.some((t) => t.toLowerCase().includes(needle))
    ) {
      return false
    }
    return true
  })
}

export function useMikrotikLogs() {
  const [entries, setEntries] = useState<readonly MikrotikLogEntry[]>([])
  const [running, setRunning] = useState(false)
  const [error, setError] = useState("")
  const [severityFilter, setSeverityFilter] = useState<LogSeverityFilter>("all")
  const [textFilter, setTextFilter] = useState("")
  const [topicFilter, setTopicFilter] = useState("")
  const [pollSeconds, setPollSeconds] = useState<number>(DEFAULT_POLL_SECONDS)
  const entriesRef = useRef<readonly MikrotikLogEntry[]>([])
  const runningRef = useRef(running)
  const pollSecondsRef = useRef(pollSeconds)
  /** Profile whose stream this instance currently holds — `stop` needs it
      because the backend keys log streams per profile. */
  const activeProfileRef = useRef<number | null>(null)

  useEffect(() => {
    runningRef.current = running
  }, [running])

  useEffect(() => {
    pollSecondsRef.current = pollSeconds
  }, [pollSeconds])

  // Unmount while streaming: cancel the backend task. The stream is
  // live-only, so nothing needs saving. The view is the stream's owner:
  // leaving the MikroTik view (or switching devices) ends its log stream.
  useEffect(() => {
    return () => {
      if (runningRef.current && activeProfileRef.current !== null) {
        void mikrotikLogStop(activeProfileRef.current)
      }
    }
  }, [])

  const handleStatus = useCallback((event: MikrotikLogStatusEvent) => {
    switch (event.event) {
      case "started":
        setRunning(true)
        setError("")
        break
      case "stopped":
        setRunning(false)
        break
      case "warning":
      case "error":
        setError(event.message)
        if (event.event === "error") {
          activeProfileRef.current = null
          setRunning(false)
        }
        break
    }
  }, [])

  const start = useCallback(
    async (profileId: number) => {
      setError("")
      try {
        const handleEvent = (event: MikrotikLogEvent) => {
          if (event.event !== "entries") return
          // Newest on top: entries arrive oldest-first from the router, so a
          // batch is reversed before it is prepended to the buffer.
          const next = [...[...event.entries].reverse(), ...entriesRef.current]
          entriesRef.current =
            next.length > LOG_BUFFER_CAP ? next.slice(0, LOG_BUFFER_CAP) : next
          setEntries(entriesRef.current)
        }
        await mikrotikLogStart(profileId, pollSecondsRef.current, handleEvent, handleStatus)
        activeProfileRef.current = profileId
        entriesRef.current = []
        setEntries([])
        setRunning(true)
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [handleStatus],
  )

  /// Apply a new poll cadence: if the stream is live, restart it so the
  /// choice takes effect immediately instead of on the next manual start.
  const setPollSecondsLive = useCallback(
    (seconds: number) => {
      setPollSeconds(seconds)
      pollSecondsRef.current = seconds
    },
    [],
  )

  const restart = useCallback(
    async (profileId: number) => {
      try {
        if (activeProfileRef.current !== null) {
          await mikrotikLogStop(activeProfileRef.current)
        }
      } catch {
        // Already terminal — the Error status event reported it.
      }
      setRunning(false)
      await start(profileId)
    },
    [start],
  )

  const stop = useCallback(async () => {
    try {
      if (activeProfileRef.current !== null) {
        await mikrotikLogStop(activeProfileRef.current)
      }
    } catch {
      // The stream may already have terminated on its own — the terminal
      // Error status event already reported the cause.
    }
    activeProfileRef.current = null
    setRunning(false)
  }, [])

  const clear = useCallback(() => {
    entriesRef.current = []
    setEntries([])
  }, [])

  const topics = useMemo(() => {
    const seen = new Set<string>()
    for (const entry of entries) {
      for (const topic of entry.topics) seen.add(topic)
    }
    return [...seen].sort((a, b) => a.localeCompare(b))
  }, [entries])

  const filtered = useMemo(
    () => filterLogEntries(entries, severityFilter, textFilter, topicFilter),
    [entries, severityFilter, textFilter, topicFilter],
  )

  return {
    entries: filtered,
    totalCount: entries.length,
    topics,
    running,
    error,
    severityFilter,
    setSeverityFilter,
    textFilter,
    setTextFilter,
    topicFilter,
    setTopicFilter,
    pollSeconds,
    setPollSeconds: setPollSecondsLive,
    start,
    restart,
    stop,
    clear,
  }
}
