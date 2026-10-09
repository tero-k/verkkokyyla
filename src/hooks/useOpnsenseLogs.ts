import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { opnsenseLogStart, opnsenseLogStop } from "../lib/ipc"
import {
  opnsenseErrorPayload,
  type OpnsenseLogEvent,
  type OpnsenseLogStatusEvent,
} from "../lib/types"

/** Visible buffer cap, mirroring the MikroTik log panel. */
export const OPNsense_LOG_BUFFER_CAP = 500

export type OpnsenseLogTab = "all" | "blocked" | "allowed"

/** Client-side filter over the capped buffer. Pure and exported for tests.
    "pass" maps to allowed, "block" (reject included) to blocked, anything
    else is "other" and appears in the All tab only. */
export function filterOpnsenseLogEntries(
  entries: readonly OpnsenseLogEvent[],
  tab: OpnsenseLogTab,
  text: string,
): OpnsenseLogEvent[] {
  const needle = text.trim().toLowerCase()
  return entries.filter((entry) => {
    if (tab === "blocked" && entry.action !== "block") return false
    if (tab === "allowed" && entry.action !== "pass") return false
    if (needle === "") return true
    return (
      entry.action.toLowerCase().includes(needle) ||
      (entry.timestamp?.toLowerCase().includes(needle) ?? false) ||
      (entry.interfaceName?.toLowerCase().includes(needle) ?? false) ||
      (entry.dir?.toLowerCase().includes(needle) ?? false) ||
      (entry.proto?.toLowerCase().includes(needle) ?? false) ||
      (entry.src?.toLowerCase().includes(needle) ?? false) ||
      (entry.srcPort?.toLowerCase().includes(needle) ?? false) ||
      (entry.dst?.toLowerCase().includes(needle) ?? false) ||
      (entry.dstPort?.toLowerCase().includes(needle) ?? false) ||
      (entry.label?.toLowerCase().includes(needle) ?? false)
    )
  })
}

export function useOpnsenseLogs() {
  const [entries, setEntries] = useState<readonly OpnsenseLogEvent[]>([])
  const [running, setRunning] = useState(false)
  const [error, setError] = useState("")
  const [tab, setTab] = useState<OpnsenseLogTab>("all")
  const [textFilter, setTextFilter] = useState("")
  const entriesRef = useRef<readonly OpnsenseLogEvent[]>([])
  const entriesByProfileRef = useRef(new Map<number, readonly OpnsenseLogEvent[]>())
  const selectedProfileRef = useRef<number | null>(null)
  /** Profile whose stream this instance currently holds — `stop` needs it
      because the backend keys log streams per profile. */
  const activeProfileRef = useRef<number | null>(null)
  /** Ignore late events from a stream after stop/profile switch. */
  const generationRef = useRef(0)
  const pendingStopsRef = useRef(new Map<number, Promise<void>>())

  // Unmount while streaming: cancel the backend task. Route changes keep
  // this view mounted; a profile switch stops only the old stream.
  useEffect(() => {
    return () => {
      generationRef.current += 1
      const profileId = activeProfileRef.current
      activeProfileRef.current = null
      if (profileId !== null) void opnsenseLogStop(profileId)
    }
  }, [])

  const stopBackend = useCallback((profileId: number): Promise<void> => {
    const pending = opnsenseLogStop(profileId).catch(() => undefined)
    pendingStopsRef.current.set(profileId, pending)
    void pending.finally(() => {
      if (pendingStopsRef.current.get(profileId) === pending) pendingStopsRef.current.delete(profileId)
    })
    return pending
  }, [])

  const selectProfile = useCallback((profileId: number | null) => {
    if (selectedProfileRef.current === profileId) return
    generationRef.current += 1
    const previous = activeProfileRef.current
    activeProfileRef.current = null
    selectedProfileRef.current = profileId
    entriesRef.current = profileId === null ? [] : (entriesByProfileRef.current.get(profileId) ?? [])
    setEntries(entriesRef.current)
    setRunning(false)
    setError("")
    if (previous !== null) void stopBackend(previous)
  }, [stopBackend])

  const start = useCallback(async (profileId: number) => {
    if (activeProfileRef.current !== null) return
    selectedProfileRef.current = profileId
    const generation = ++generationRef.current
    activeProfileRef.current = profileId
    entriesRef.current = entriesByProfileRef.current.get(profileId) ?? []
    setEntries(entriesRef.current)
    setRunning(true)
    setError("")
    try {
      await pendingStopsRef.current.get(profileId)
      if (generationRef.current !== generation) return
      const handleEvent = (event: OpnsenseLogEvent) => {
        if (generationRef.current !== generation || event.profileId !== profileId) return
        // Newest on top: prepend each event to the capped buffer.
        const next = [event, ...entriesRef.current]
        entriesRef.current =
          next.length > OPNsense_LOG_BUFFER_CAP ? next.slice(0, OPNsense_LOG_BUFFER_CAP) : next
        entriesByProfileRef.current.set(profileId, entriesRef.current)
        setEntries(entriesRef.current)
      }
      const handleStatus = (event: OpnsenseLogStatusEvent) => {
        if (generationRef.current !== generation || event.profileId !== profileId) return
        switch (event.event) {
          case "started":
            setRunning(true)
            setError("")
            break
          case "stopped":
            generationRef.current += 1
            activeProfileRef.current = null
            setRunning(false)
            break
          case "warning":
            setError(event.message)
            break
          case "error":
            generationRef.current += 1
            activeProfileRef.current = null
            setRunning(false)
            setError(event.message)
            break
        }
      }
      await opnsenseLogStart(profileId, handleEvent, handleStatus)
      if (generationRef.current !== generation) {
        // Stop raced with the start command; close the newly opened stream.
        await opnsenseLogStop(profileId).catch(() => undefined)
      }
    } catch (err) {
      if (generationRef.current === generation) {
        activeProfileRef.current = null
        setRunning(false)
        setError(opnsenseErrorPayload(err).message)
      }
    }
  }, [])

  const stop = useCallback(async () => {
    generationRef.current += 1
    const profileId = activeProfileRef.current
    activeProfileRef.current = null
    setRunning(false)
    if (profileId === null) return
    await stopBackend(profileId)
  }, [stopBackend])

  const clear = useCallback(() => {
    entriesRef.current = []
    if (selectedProfileRef.current !== null) entriesByProfileRef.current.set(selectedProfileRef.current, [])
    setEntries([])
  }, [])

  const filtered = useMemo(
    () => filterOpnsenseLogEntries(entries, tab, textFilter),
    [entries, tab, textFilter],
  )

  return {
    entries: filtered,
    totalCount: entries.length,
    running,
    error,
    tab,
    setTab,
    textFilter,
    setTextFilter,
    start,
    stop,
    clear,
    selectProfile,
  }
}
