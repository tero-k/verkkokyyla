import { useCallback, useEffect, useRef, useState } from "react"
import {
  mikrotikBackup,
  mikrotikCheckUpdates,
  mikrotikDeleteSession,
  mikrotikFetchChangelog,
  mikrotikListProfiles,
  mikrotikListSessions,
  mikrotikLoadSession,
  mikrotikStart,
  mikrotikStop,
} from "../lib/ipc"
import {
  buildMikrotikHistory,
  pushMikrotikRatePoint,
  type MikrotikRateSeries,
} from "../lib/mikrotikSeries"
import type { BackupResultDto, MikrotikBridgeVlanDto, MikrotikChangelogDto, MikrotikFirmwareStatus, MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSensorDto, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikStatusEvent, MikrotikUpdateStatus, MikrotikVlanDto } from "../lib/types"

type RunBackupRequest = { readonly profileId: number; readonly destinationDir: string; readonly backupName: string; readonly password?: string; readonly includeRsc: boolean; readonly overwrite: boolean }

function assertNever(value: never): never {
  throw new Error(`unexpected Mikrotik status event: ${JSON.stringify(value)}`)
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  if (typeof err === "object" && err !== null && "message" in err) {
    const message: unknown = err.message
    if (typeof message === "string") return message
  }
  return String(err)
}

export function useMikrotik() {
  const [profiles, setProfiles] = useState<readonly MikrotikProfile[]>([]), [selectedProfile, setSelectedProfile] = useState<MikrotikProfile | null>(null)
  const [running, setRunning] = useState(false), [latestSnapshot, setLatestSnapshot] = useState<MikrotikSnapshotEvent | null>(null)
  const [snapshotHistory, setSnapshotHistory] = useState<readonly MikrotikSnapshotEvent[]>([]), [rateSeries, setRateSeries] = useState<MikrotikRateSeries>({})
  const [vlans, setVlans] = useState<readonly MikrotikVlanDto[]>([]), [bridgeVlans, setBridgeVlans] = useState<readonly MikrotikBridgeVlanDto[]>([])
  const [sensors, setSensors] = useState<readonly MikrotikSensorDto[]>([]), [updateStatus, setUpdateStatus] = useState<MikrotikUpdateStatus | null>(null)
  const [firmwareStatus, setFirmwareStatus] = useState<MikrotikFirmwareStatus | null>(null), [sessions, setSessions] = useState<readonly MikrotikSessionSummaryDto[]>([])
  const [loadedSession, setLoadedSession] = useState<MikrotikLoadedSessionDto | null>(null)
  const [error, setError] = useState("")

  const latestSnapshotRef = useRef<MikrotikSnapshotEvent | null>(null), snapshotHistoryRef = useRef<readonly MikrotikSnapshotEvent[]>([])
  const rateSeriesRef = useRef<MikrotikRateSeries>({}), vlansRef = useRef<readonly MikrotikVlanDto[]>([])
  const bridgeVlansRef = useRef<readonly MikrotikBridgeVlanDto[]>([]), sensorsRef = useRef<readonly MikrotikSensorDto[]>([])
  const pendingRef = useRef<MikrotikSnapshotEvent[]>([]), flushingRef = useRef(false), flushRef = useRef(() => {}), runningRef = useRef(running)

  useEffect(() => {
    runningRef.current = running
  }, [running])

  const refreshProfiles = useCallback(async () => {
    try {
      const nextProfiles = await mikrotikListProfiles()
      setProfiles(nextProfiles)
      setSelectedProfile((current) => current ?? nextProfiles[0] ?? null)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await mikrotikListSessions())
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  useEffect(() => {
    void refreshProfiles()
    void refreshSessions()
  }, [refreshProfiles, refreshSessions])

  useEffect(() => {
    return () => {
      if (runningRef.current) {
        void mikrotikStop()
      }
    }
  }, [])

  const resetLiveState = useCallback(() => {
    latestSnapshotRef.current = null; snapshotHistoryRef.current = []; rateSeriesRef.current = {}
    vlansRef.current = []; bridgeVlansRef.current = []; sensorsRef.current = []; pendingRef.current = []
    flushingRef.current = false
    setLatestSnapshot(null); setSnapshotHistory([]); setRateSeries({})
    setVlans([]); setBridgeVlans([]); setSensors([])
  }, [])

  const applySnapshot = useCallback((snapshot: MikrotikSnapshotEvent) => {
    latestSnapshotRef.current = snapshot
    snapshotHistoryRef.current = [...snapshotHistoryRef.current, snapshot]
    rateSeriesRef.current = pushMikrotikRatePoint(rateSeriesRef.current, snapshot)
    if (snapshot.vlans !== null) vlansRef.current = snapshot.vlans
    if (snapshot.bridgeVlans !== null) bridgeVlansRef.current = snapshot.bridgeVlans
    if (snapshot.sensors !== null) sensorsRef.current = snapshot.sensors
  }, [])

  const publishSnapshotState = useCallback(() => {
    setLatestSnapshot(latestSnapshotRef.current)
    setSnapshotHistory(snapshotHistoryRef.current)
    setRateSeries(rateSeriesRef.current)
    setVlans(vlansRef.current)
    setBridgeVlans(bridgeVlansRef.current)
    setSensors(sensorsRef.current)
  }, [])

  const flush = useCallback(() => {
    flushingRef.current = false
    const batch = pendingRef.current
    pendingRef.current = []
    if (batch.length === 0) return

    for (const snapshot of batch) {
      applySnapshot(snapshot)
    }
    publishSnapshotState()
  }, [applySnapshot, publishSnapshotState])

  flushRef.current = flush

  const handleStatus = useCallback(
    (event: MikrotikStatusEvent) => {
      switch (event.event) {
        case "started":
          setRunning(true)
          break
        case "stopped":
        case "cancelled":
          setRunning(false)
          void refreshSessions()
          break
        case "warning":
          setError(event.message)
          break
        case "error":
          setError(event.message)
          setRunning(false)
          void refreshSessions()
          break
        case "version-firmware":
          setUpdateStatus(event.updateStatus)
          setFirmwareStatus(event.firmwareStatus)
          break
        default:
          assertNever(event)
      }
    },
    [refreshSessions],
  )

  const selectProfile = useCallback((profile: MikrotikProfile | null) => {
    setSelectedProfile(profile)
    setError("")
  }, [])

  const start = useCallback(async () => {
    if (selectedProfile === null) {
      setError("Select a MikroTik profile before starting monitoring")
      return
    }
    setError("")
    try {
      const handleEvent = (event: MikrotikSnapshotEvent) => {
        pendingRef.current.push(event)
        if (!flushingRef.current) {
          flushingRef.current = true
          requestAnimationFrame(() => flushRef.current())
        }
      }
      await mikrotikStart(selectedProfile.id, handleEvent, handleStatus)
      resetLiveState()
      setLoadedSession(null)
      setUpdateStatus(null)
      setFirmwareStatus(null)
      setRunning(true)
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [handleStatus, resetLiveState, selectedProfile])

  const stop = useCallback(async () => {
    try {
      await mikrotikStop()
      setRunning(false)
      void refreshSessions()
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [refreshSessions])

  const loadSession = useCallback(
    async (id: number) => {
      if (running) return
      try {
        const loaded = await mikrotikLoadSession(id)
        const history = buildMikrotikHistory(loaded.snapshots, loaded.session)
        latestSnapshotRef.current = history.latestSnapshot; snapshotHistoryRef.current = history.snapshotHistory; rateSeriesRef.current = history.rateSeries
        vlansRef.current = history.vlans; bridgeVlansRef.current = history.bridgeVlans; sensorsRef.current = history.sensors
        setLoadedSession(loaded)
        setLatestSnapshot(history.latestSnapshot); setSnapshotHistory(history.snapshotHistory); setRateSeries(history.rateSeries)
        setVlans(history.vlans); setBridgeVlans(history.bridgeVlans); setSensors(history.sensors)
        setUpdateStatus(history.updateStatus); setFirmwareStatus(history.firmwareStatus)
        setError("")
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [running],
  )

  const deleteSession = useCallback(
    async (id: number) => {
      if (running) return
      try {
        await mikrotikDeleteSession(id)
        if (loadedSession?.session.id === id) {
          setLoadedSession(null)
          resetLiveState()
        }
        await refreshSessions()
      } catch (err) {
        setError(errorMessage(err))
      }
    },
    [loadedSession, refreshSessions, resetLiveState, running],
  )

  const checkUpdates = useCallback(async () => {
    if (selectedProfile === null) return null
    try {
      const result = await mikrotikCheckUpdates(selectedProfile.id)
      setUpdateStatus(result.updateStatus)
      setFirmwareStatus(result.firmwareStatus)
      setError("")
      return result
    } catch (err) {
      setError(errorMessage(err))
      return null
    }
  }, [selectedProfile])

  const fetchChangelog = useCallback(async (version: string): Promise<MikrotikChangelogDto | null> => {
    try {
      setError("")
      return await mikrotikFetchChangelog(version)
    } catch (err) {
      setError(errorMessage(err))
      return null
    }
  }, [])

  const runBackup = useCallback(async (request: RunBackupRequest): Promise<BackupResultDto | null> => {
    try {
      setError("")
      return await mikrotikBackup(request.profileId, request.destinationDir, request.backupName, request.password, request.includeRsc, request.overwrite)
    } catch (err) {
      setError(errorMessage(err))
      return null
    }
  }, [])

  return { profiles, selectedProfile, running, latestSnapshot, snapshotHistory, rateSeries, vlans, bridgeVlans, sensors, updateStatus, firmwareStatus, sessions, loadedSession, error, selectProfile, start, stop, loadSession, deleteSession, refreshProfiles, checkUpdates, fetchChangelog, runBackup }
}
