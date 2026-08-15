import { useCallback, useEffect, useRef, useState } from "react"
import { TABLE_ROW_CAP } from "../lib/constants"
import {
  deleteSession as deleteSessionCommand,
  getSnapshot,
  listSessions,
  loadSession,
  startSession,
  stopSession,
} from "../lib/ipc"
import { computeSnapshot } from "../lib/snapshot"
import { TableBuffer } from "../lib/tableBuffer"
import type {
  Family,
  LoadedSessionDto,
  ProbeEvent,
  ProbeRow,
  SessionSummaryDto,
  SnapshotDto,
  StartInfoDto,
  StatusEvent,
} from "../lib/types"

type ViewMode = "live" | "past"

function assertNever(value: never): never {
  throw new Error(`unexpected status event: ${JSON.stringify(value)}`)
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  return String(err)
}

export function usePingSession() {
  const [target, setTarget] = useState("")
  const [family, setFamily] = useState<Family>("auto")
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [pausedError, setPausedError] = useState("")
  const [status, setStatus] = useState("")
  const [startInfo, setStartInfo] = useState<StartInfoDto | null>(null)
  const [snapshot, setSnapshot] = useState<SnapshotDto | null>(null)
  const [tableRows, setTableRows] = useState<readonly ProbeRow[]>([])
  const [allProbes, setAllProbes] = useState<readonly ProbeRow[]>([])
  const [sessions, setSessions] = useState<readonly SessionSummaryDto[]>([])
  const [viewMode, setViewMode] = useState<ViewMode>("live")
  const [pastSession, setPastSession] = useState<LoadedSessionDto | null>(null)

  const tableBufferRef = useRef(new TableBuffer(TABLE_ROW_CAP))
  const allProbesRef = useRef<ProbeRow[]>([])
  const pendingRef = useRef<ProbeEvent[]>([])
  const flushingRef = useRef(false)
  const flushRef = useRef(() => {})
  const isRunningRef = useRef(isRunning)

  useEffect(() => {
    isRunningRef.current = isRunning
  }, [isRunning])

  const refreshSnapshot = useCallback(async () => {
    try {
      const snap = await getSnapshot()
      setSnapshot(snap)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const refreshList = useCallback(async () => {
    try {
      const rows = await listSessions()
      setSessions(rows)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const clearErrors = useCallback(() => {
    setError("")
    setPausedError("")
  }, [])

  useEffect(() => {
    void refreshList()
  }, [refreshList])

  const flush = useCallback(() => {
    flushingRef.current = false
    const batch = pendingRef.current
    pendingRef.current = []
    if (batch.length === 0) return

    const buffer = tableBufferRef.current
    for (const event of batch) {
      const row: ProbeRow = {
        seq: event.seq,
        rttMs: event.rttMs,
        lost: event.lost,
        at: event.at,
      }
      buffer.add(row)
      allProbesRef.current.push(row)
    }

    setTableRows(buffer.all())
    setAllProbes([...allProbesRef.current])
    void refreshSnapshot()
  }, [refreshSnapshot])

  flushRef.current = flush

  const handleStatus = useCallback((event: StatusEvent) => {
    switch (event.event) {
      case "engine-selected":
        setStatus(
          event.fallback
            ? `Engine: ${event.engine} (fallback)`
            : `Engine: ${event.engine}`,
        )
        break
      case "error":
        if (isRunningRef.current) {
          setPausedError(event.message)
        } else {
          setError(event.message)
        }
        break
      case "session-stopped":
        setStatus(
          `Stopped after ${event.probeCount} probes (${event.lossCount} lost)`,
        )
        break
      default:
        assertNever(event)
    }
  }, [])

  const start = useCallback(async () => {
    clearErrors()
    setStatus("")
    try {
      const handleProbe = (event: ProbeEvent) => {
        pendingRef.current.push(event)
        if (!flushingRef.current) {
          flushingRef.current = true
          requestAnimationFrame(() => flushRef.current())
        }
      }

      const info = await startSession(
        target,
        family,
        handleProbe,
        handleStatus,
      )

      tableBufferRef.current.clear()
      allProbesRef.current = []
      pendingRef.current = []
      flushingRef.current = false
      setTableRows([])
      setAllProbes([])
      setSnapshot(null)
      setStartInfo(info)
      setIsRunning(true)
      setViewMode("live")
      setPastSession(null)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [clearErrors, family, handleStatus, target])

  const stop = useCallback(async () => {
    try {
      await stopSession()
      setIsRunning(false)
      setPausedError("")
      void refreshSnapshot()
      void refreshList()
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [refreshList, refreshSnapshot])

  const retry = useCallback(async () => {
    if (isRunning) {
      await stop()
    }
    await start()
  }, [isRunning, start, stop])

  const openSession = useCallback(
    async (id: number) => {
      if (isRunning) return
      try {
        const loaded = await loadSession(id)
        const probes = loaded.probes.map((probe) => ({
          seq: probe.seq,
          rttMs: probe.rttMs,
          lost: probe.loss,
          at: probe.at,
        }))
        setPastSession(loaded)
        setViewMode("past")
        setAllProbes(probes)
        setTableRows(probes.slice(-TABLE_ROW_CAP))
        setSnapshot(computeSnapshot(probes))
        setStartInfo(null)
        setError("")
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning],
  )

  const deleteSession = useCallback(
    async (id: number) => {
      if (isRunning) return
      if (!window.confirm("Delete this session?")) return
      try {
        await deleteSessionCommand(id)
        if (pastSession?.session.id === id) {
          setPastSession(null)
          setViewMode("live")
          setTableRows([])
          setAllProbes([])
          setSnapshot(null)
        }
        void refreshList()
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning, pastSession, refreshList],
  )

  return {
    target,
    setTarget,
    family,
    setFamily,
    isRunning,
    error,
    pausedError,
    status,
    startInfo,
    snapshot,
    tableRows,
    allProbes,
    sessions,
    viewMode,
    pastSession,
    start,
    stop,
    retry,
    openSession,
    deleteSession,
  }
}
