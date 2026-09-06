import { useCallback, useEffect, useRef, useState } from "react"
import {
  deleteScan,
  listInterfaces,
  listScans,
  loadScan,
  startScan,
  stopScan,
} from "../lib/ipc"
import {
  applyHostEvents,
  buildDefaultCidr,
  getPrimaryInterface,
} from "../lib/lanScan"
import type {
  InterfaceDto,
  LoadedScanDto,
  ScanEvent,
  ScanHostDto,
  ScanStatusEvent,
  ScanSummaryDto,
} from "../lib/types"

type ViewMode = "live" | "past"

function assertNever(value: never): never {
  throw new Error(`unexpected scan status event: ${JSON.stringify(value)}`)
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  if (typeof err === "object" && err !== null && "message" in err) {
    if (typeof err.message === "string") return err.message
  }
  return String(err)
}

export function useLanScan() {
  const [interfaces, setInterfaces] = useState<readonly InterfaceDto[]>([])
  const [selectedInterface, setSelectedInterface] =
    useState<InterfaceDto | null>(null)
  const [cidr, setCidr] = useState("")
  const [tcpFallback, setTcpFallback] = useState(true)
  const [portsEnabled, setPortsEnabled] = useState(false)
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [progress, setProgress] = useState<{
    readonly done: number
    readonly total: number
  } | null>(null)
  const [hosts, setHosts] = useState<readonly ScanHostDto[]>([])
  const [pastScans, setPastScans] = useState<readonly ScanSummaryDto[]>([])
  const [viewMode, setViewMode] = useState<ViewMode>("live")
  const [pastScan, setPastScan] = useState<LoadedScanDto | null>(null)

  const selectInterface = useCallback((iface: InterfaceDto) => {
    setSelectedInterface(iface)
    setCidr(buildDefaultCidr(iface))
  }, [])

  const hostsRef = useRef<readonly ScanHostDto[]>([])
  const pendingRef = useRef<ScanEvent[]>([])
  const flushingRef = useRef(false)
  const flushRef = useRef(() => {})
  const isRunningRef = useRef(isRunning)

  useEffect(() => {
    isRunningRef.current = isRunning
  }, [isRunning])

  const refreshScans = useCallback(async () => {
    try {
      const scans = await listScans()
      setPastScans(scans)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  useEffect(() => {
    void refreshScans()
  }, [refreshScans])

  useEffect(() => {
    async function load() {
      try {
        const ifaces = await listInterfaces()
        setInterfaces(ifaces)
        const primary = getPrimaryInterface(ifaces)
        if (primary !== null) {
          setSelectedInterface(primary)
          setCidr(buildDefaultCidr(primary))
        }
      } catch (err) {
        setError(errorMessage(err))
      }
    }
    void load()
  }, [])

  useEffect(() => {
    return () => {
      if (isRunningRef.current) {
        void stopScan()
      }
    }
  }, [])

  const flush = useCallback(() => {
    flushingRef.current = false
    const batch = pendingRef.current
    pendingRef.current = []
    if (batch.length === 0) return

    const nextRows = applyHostEvents(hostsRef.current, batch)
    hostsRef.current = nextRows
    setHosts(nextRows)
  }, [])

  flushRef.current = flush

  const handleStatus = useCallback(
    (event: ScanStatusEvent) => {
      switch (event.event) {
        case "engine":
          setStatus(
            `Engine: ${event.engine} (TCP fallback ${event.tcpFallback ? "on" : "off"})`,
          )
          break
        case "progress":
          setProgress({ done: event.done, total: event.total })
          setStatus(`Scanned ${event.done} of ${event.total} hosts`)
          break
        case "stopped":
          setStatus(`Scan stopped: ${event.hostCount} hosts found`)
          setIsRunning(false)
          void refreshScans()
          break
        case "completed":
          setStatus(`Scan completed: ${event.hostCount} hosts found`)
          setIsRunning(false)
          void refreshScans()
          break
        case "error":
          setError(event.message)
          setIsRunning(false)
          break
        default:
          assertNever(event)
      }
    },
    [refreshScans],
  )

  const start = useCallback(async () => {
    if (selectedInterface === null) {
      setError("Select a network interface")
      return
    }
    if (cidr.trim().length === 0) {
      setError("Enter a CIDR range")
      return
    }

    setError("")
    setStatus("")
    setProgress(null)
    setPastScan(null)
    setViewMode("live")
    hostsRef.current = []
    pendingRef.current = []
    flushingRef.current = false
    setHosts([])

    try {
      const handleEvent = (event: ScanEvent) => {
        pendingRef.current.push(event)
        if (!flushingRef.current) {
          flushingRef.current = true
          requestAnimationFrame(() => flushRef.current())
        }
      }

      await startScan(
        selectedInterface.name,
        cidr.trim(),
        tcpFallback,
        portsEnabled,
        handleEvent,
        handleStatus,
      )

      setIsRunning(true)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [cidr, handleStatus, portsEnabled, selectedInterface, tcpFallback])

  const stop = useCallback(async () => {
    try {
      await stopScan()
      setIsRunning(false)
      void refreshScans()
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [refreshScans])

  const openScan = useCallback(
    async (id: number) => {
      if (isRunning) return
      try {
        const loaded = await loadScan(id)
        setPastScan(loaded)
        hostsRef.current = loaded.hosts
        setHosts(loaded.hosts)
        setViewMode("past")
        setError("")
        setStatus("")
        setProgress(null)
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning],
  )

  const deleteScanById = useCallback(
    async (id: number) => {
      if (isRunning) return
      try {
        await deleteScan(id)
        if (pastScan?.scan.id === id) {
          setPastScan(null)
          setViewMode("live")
          hostsRef.current = []
          setHosts([])
        }
        void refreshScans()
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [isRunning, pastScan, refreshScans],
  )

  return {
    interfaces,
    selectedInterface,
    selectInterface,
    cidr,
    setCidr,
    tcpFallback,
    setTcpFallback,
    portsEnabled,
    setPortsEnabled,
    isRunning,
    error,
    status,
    progress,
    hosts,
    pastScans,
    viewMode,
    pastScan,
    start,
    stop,
    openScan,
    deleteScan: deleteScanById,
  }
}
