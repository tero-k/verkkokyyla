import { useCallback, useEffect, useRef, useState } from "react"
import { deleteTrace, listTraces, loadTrace, startTrace, stopTrace } from "../lib/ipc"
import { validateTarget } from "../lib/validate"
import { applyHostnameEvent, applyHopEvent, compareTraceHops } from "../lib/traceHops"
import type {
  ComparedHopRow,
  Family,
  LoadedTraceDto,
  TraceEvent,
  TraceHopRow,
  TraceStatusEvent,
  TraceSummaryDto,
} from "../lib/types"

type ViewMode = "live" | "past" | "compare"

function assertNever(value: never): never {
  throw new Error(`unexpected trace status event: ${JSON.stringify(value)}`)
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  if (typeof err === "object" && err !== null && "message" in err) {
    if (typeof err.message === "string") return err.message
  }
  return String(err)
}

export function useTraceroute() {
  const [target, setTarget] = useState("")
  const [family, setFamily] = useState<Family>("auto")
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [hops, setHops] = useState<readonly TraceHopRow[]>([])
  const [pastTraces, setPastTraces] = useState<readonly TraceSummaryDto[]>([])
  const [viewMode, setViewMode] = useState<ViewMode>("live")
  const [pastTrace, setPastTrace] = useState<LoadedTraceDto | null>(null)
  const [isCompareSelecting, setIsCompareSelecting] = useState(false)
  const [compareSelection, setCompareSelection] = useState<readonly number[]>([])
  const [compareA, setCompareA] = useState<LoadedTraceDto | null>(null)
  const [compareB, setCompareB] = useState<LoadedTraceDto | null>(null)
  const [compareDiff, setCompareDiff] = useState<readonly ComparedHopRow[]>([])

  const hopsRef = useRef<TraceHopRow[]>([])
  const pendingRef = useRef<TraceEvent[]>([])
  const flushingRef = useRef(false)
  const flushRef = useRef(() => {})
  const isRunningRef = useRef(isRunning)

  useEffect(() => {
    isRunningRef.current = isRunning
  }, [isRunning])

  const refreshTraces = useCallback(async () => {
    try {
      const traces = await listTraces()
      setPastTraces(traces)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const clearCompare = useCallback(() => {
    setIsCompareSelecting(false)
    setCompareSelection([])
    setCompareA(null)
    setCompareB(null)
    setCompareDiff([])
    setViewMode((current) => (current === "compare" ? "live" : current))
  }, [])

  const startCompareSelection = useCallback(() => {
    setIsCompareSelecting(true)
    setCompareA(null)
    setCompareB(null)
    setCompareDiff([])
    setViewMode((current) => (current === "compare" ? "live" : current))
  }, [])

  const toggleCompareSelection = useCallback((id: number) => {
    setCompareSelection((selected) => {
      if (selected.includes(id)) {
        return selected.filter((existing) => existing !== id)
      }
      if (selected.length < 2) {
        return [...selected, id]
      }
      // Replace the oldest selection so the latest two are kept.
      return [selected[1], id]
    })
  }, [])

  const compareSelected = useCallback(async () => {
    if (compareSelection.length !== 2) return
    setError("")
    setStatus("")
    try {
      const [a, b] = await Promise.all([
        loadTrace(compareSelection[0]),
        loadTrace(compareSelection[1]),
      ])
      setIsCompareSelecting(false)
      setCompareA(a)
      setCompareB(b)
      setCompareDiff(compareTraceHops(a.hops, b.hops))
      setPastTrace(null)
      hopsRef.current = []
      setHops([])
      setViewMode("compare")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [compareSelection])

  useEffect(() => {
    void refreshTraces()
  }, [refreshTraces])

  useEffect(() => {
    return () => {
      if (isRunningRef.current) {
        void stopTrace()
      }
    }
  }, [])

  const flush = useCallback(() => {
    flushingRef.current = false
    const batch = pendingRef.current
    pendingRef.current = []
    if (batch.length === 0) return

    let nextRows = hopsRef.current
    for (const event of batch) {
      if (event.event === "hop") {
        nextRows = applyHopEvent(nextRows, event)
      } else {
        nextRows = applyHostnameEvent(nextRows, event)
      }
    }

    hopsRef.current = nextRows
    setHops(nextRows)
  }, [])

  flushRef.current = flush

  const handleStatus = useCallback(
    (event: TraceStatusEvent) => {
      switch (event.event) {
        case "completed":
          setStatus(
            `Trace completed after ${event.hopCount} hops${event.reachedTarget ? " (reached target)" : ""}`,
          )
          setIsRunning(false)
          void refreshTraces()
          break
        case "cancelled":
          setStatus(`Trace cancelled after ${event.hopCount} hops`)
          setIsRunning(false)
          void refreshTraces()
          break
        case "error":
          setError(event.message)
          setIsRunning(false)
          break
        default:
          assertNever(event)
      }
    },
    [refreshTraces],
  )

  const start = useCallback(async () => {
    const validation = validateTarget(target)
    if (!validation.ok) {
      setError(validation.error)
      return
    }

    setError("")
    setStatus("")
    try {
      const handleEvent = (event: TraceEvent) => {
        pendingRef.current.push(event)
        if (!flushingRef.current) {
          flushingRef.current = true
          requestAnimationFrame(() => flushRef.current())
        }
      }

      await startTrace(validation.value, family, handleEvent, handleStatus)

      hopsRef.current = []
      pendingRef.current = []
      flushingRef.current = false
      setHops([])
      setPastTrace(null)
      clearCompare()
      setViewMode("live")
      setIsRunning(true)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [family, handleStatus, target])

  const stop = useCallback(async () => {
    try {
      await stopTrace()
      setIsRunning(false)
      void refreshTraces()
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [refreshTraces])

  const openTrace = useCallback(
    async (id: number) => {
      if (isRunning) return
      try {
        const loaded = await loadTrace(id)
        setPastTrace(loaded)
        clearCompare()
        setViewMode("past")
        hopsRef.current = [...loaded.hops]
        setHops(loaded.hops)
        setError("")
        setStatus("")
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning],
  )

  const deleteTraceById = useCallback(
    async (id: number) => {
      if (isRunning) return
      if (!window.confirm("Delete this trace?")) return
      try {
        await deleteTrace(id)
        if (pastTrace?.trace.id === id) {
          setPastTrace(null)
          setViewMode("live")
          hopsRef.current = []
          setHops([])
        }
        setCompareSelection((selected) =>
          selected.filter((existing) => existing !== id),
        )
        if (compareA?.trace.id === id || compareB?.trace.id === id) {
          setCompareA(null)
          setCompareB(null)
          setCompareDiff([])
          setViewMode((current) =>
            current === "compare" ? "live" : current,
          )
        }
        void refreshTraces()
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning, pastTrace, compareA, compareB, refreshTraces],
  )

  return {
    target,
    setTarget,
    family,
    setFamily,
    isRunning,
    error,
    status,
    hops,
    pastTraces,
    viewMode,
    pastTrace,
    isCompareSelecting,
    compareSelection,
    compareA,
    compareB,
    compareDiff,
    startCompareSelection,
    toggleCompareSelection,
    compareSelected,
    clearCompare,
    start,
    stop,
    openTrace,
    deleteTrace: deleteTraceById,
  }
}
