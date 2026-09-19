import { useCallback, useEffect, useRef, useState } from "react"
import {
  mikrotikBackup,
  mikrotikCheckUpdates,
  mikrotikDeleteSession,
  mikrotikFetchChangelog,
  mikrotikListActive,
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
import type { BackupResultDto, MikrotikActiveSessionDto, MikrotikBridgeVlanDto, MikrotikChangelogDto, MikrotikFirmwareStatus, MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSensorDto, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikStatusEvent, MikrotikUpdateStatus, MikrotikVlanDto } from "../lib/types"

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

/** Live state of ONE monitored device. `attached` distinguishes sessions
    whose event channel this hook instance holds: after a view reload the
    backend session keeps running but its channel points at the dead hook,
    so the device shows as live-but-detached until the user reconnects. */
export type MikrotikDeviceState = {
  readonly profileId: number
  readonly sessionId: number | null
  readonly running: boolean
  readonly attached: boolean
  readonly latestSnapshot: MikrotikSnapshotEvent | null
  readonly snapshotHistory: readonly MikrotikSnapshotEvent[]
  readonly rateSeries: MikrotikRateSeries
  readonly vlans: readonly MikrotikVlanDto[]
  readonly bridgeVlans: readonly MikrotikBridgeVlanDto[]
  readonly sensors: readonly MikrotikSensorDto[]
  readonly updateStatus: MikrotikUpdateStatus | null
  readonly firmwareStatus: MikrotikFirmwareStatus | null
  readonly error: string
}

/** Mutable per-device store: the React-facing `MikrotikDeviceState` is a
    snapshot of this, published on change (same ref-batching pattern the
    single-session hook used, now keyed by profile). */
type MutableDevice = {
  sessionId: number | null
  running: boolean
  attached: boolean
  latestSnapshot: MikrotikSnapshotEvent | null
  snapshotHistory: MikrotikSnapshotEvent[]
  rateSeries: MikrotikRateSeries
  vlans: readonly MikrotikVlanDto[]
  bridgeVlans: readonly MikrotikBridgeVlanDto[]
  sensors: readonly MikrotikSensorDto[]
  updateStatus: MikrotikUpdateStatus | null
  firmwareStatus: MikrotikFirmwareStatus | null
  error: string
  pending: MikrotikSnapshotEvent[]
  flushing: boolean
}

function emptyDevice(): MutableDevice {
  return {
    sessionId: null,
    running: true,
    attached: true,
    latestSnapshot: null,
    snapshotHistory: [],
    rateSeries: {},
    vlans: [],
    bridgeVlans: [],
    sensors: [],
    updateStatus: null,
    firmwareStatus: null,
    error: "",
    pending: [],
    flushing: false,
  }
}

function deviceStateOf(profileId: number, d: MutableDevice): MikrotikDeviceState {
  return {
    profileId,
    sessionId: d.sessionId,
    running: d.running,
    attached: d.attached,
    latestSnapshot: d.latestSnapshot,
    snapshotHistory: d.snapshotHistory,
    rateSeries: d.rateSeries,
    vlans: d.vlans,
    bridgeVlans: d.bridgeVlans,
    sensors: d.sensors,
    updateStatus: d.updateStatus,
    firmwareStatus: d.firmwareStatus,
    error: d.error,
  }
}

/** History view: a loaded past session renders through the same dashboard
    fields until live monitoring (or another selection) replaces it. */
type HistoryState = {
  readonly loadedSession: MikrotikLoadedSessionDto
  readonly latestSnapshot: MikrotikSnapshotEvent | null
  readonly snapshotHistory: readonly MikrotikSnapshotEvent[]
  readonly rateSeries: MikrotikRateSeries
  readonly vlans: readonly MikrotikVlanDto[]
  readonly bridgeVlans: readonly MikrotikBridgeVlanDto[]
  readonly sensors: readonly MikrotikSensorDto[]
  readonly updateStatus: MikrotikUpdateStatus | null
  readonly firmwareStatus: MikrotikFirmwareStatus | null
}

export function useMikrotik() {
  const [profiles, setProfiles] = useState<readonly MikrotikProfile[]>([]), [selectedProfile, setSelectedProfile] = useState<MikrotikProfile | null>(null)
  const [devices, setDevices] = useState<ReadonlyMap<number, MikrotikDeviceState>>(new Map())
  const [history, setHistory] = useState<HistoryState | null>(null)
  const [sessions, setSessions] = useState<readonly MikrotikSessionSummaryDto[]>([])
  const [globalError, setGlobalError] = useState("")

  const deviceRefs = useRef(new Map<number, MutableDevice>())
  const flushRefs = useRef(new Map<number, () => void>())
  const publishRef = useRef<(profileId: number) => void>(() => {})

  const publish = useCallback((profileId: number) => {
    setDevices((prev) => {
      const next = new Map(prev)
      const device = deviceRefs.current.get(profileId)
      // Keep ended/error devices while they still carry something the
      // dashboard can show (last snapshot, error); the strip lists only
      // running ones via `activeDevices`.
      if (device === undefined || (!device.running && device.latestSnapshot === null && device.error === "")) {
        next.delete(profileId)
      } else {
        next.set(profileId, deviceStateOf(profileId, device))
      }
      return next
    })
  }, [])
  publishRef.current = publish

  const refreshProfiles = useCallback(async () => {
    try {
      const nextProfiles = await mikrotikListProfiles()
      setProfiles(nextProfiles)
      setSelectedProfile((current) => current ?? nextProfiles[0] ?? null)
    } catch (err) {
      setGlobalError(errorMessage(err))
    }
  }, [])

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await mikrotikListSessions())
    } catch (err) {
      setGlobalError(errorMessage(err))
    }
  }, [])

  useEffect(() => {
    void refreshProfiles()
    void refreshSessions()
  }, [refreshProfiles, refreshSessions])

  // Rehydrate the device switcher after a (re)mount: sessions that are
  // already live show up as running-but-detached — the channels from the
  // previous hook instance are gone, so the dashboard offers a reconnect.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const active: MikrotikActiveSessionDto[] = await mikrotikListActive()
        if (cancelled || active.length === 0) return
        for (const entry of active) {
          if (!deviceRefs.current.has(entry.profileId)) {
            const device = emptyDevice()
            device.sessionId = entry.sessionId
            device.attached = false
            deviceRefs.current.set(entry.profileId, device)
            publishRef.current(entry.profileId)
          }
        }
      } catch {
        // list_active is a best-effort convenience; failures surface on the
        // next real operation.
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const applySnapshot = useCallback((device: MutableDevice, snapshot: MikrotikSnapshotEvent) => {
    device.latestSnapshot = snapshot
    device.snapshotHistory = [...device.snapshotHistory, snapshot]
    device.rateSeries = pushMikrotikRatePoint(device.rateSeries, snapshot)
    if (snapshot.vlans !== null) device.vlans = snapshot.vlans
    if (snapshot.bridgeVlans !== null) device.bridgeVlans = snapshot.bridgeVlans
    if (snapshot.sensors !== null) device.sensors = snapshot.sensors
  }, [])

  const flush = useCallback(
    (profileId: number) => {
      const device = deviceRefs.current.get(profileId)
      if (device === undefined) return
      device.flushing = false
      const batch = device.pending
      device.pending = []
      if (batch.length === 0) return
      for (const snapshot of batch) {
        applySnapshot(device, snapshot)
      }
      publish(profileId)
    },
    [applySnapshot, publish],
  )

  const handleStatus = useCallback(
    (profileId: number, event: MikrotikStatusEvent) => {
      const device = deviceRefs.current.get(profileId)
      if (device === undefined) return
      // `warning` is the only variant without a session id. Events carrying
      // one are ignored when they belong to a previous session — a delayed
      // terminal event from a restarted device's old session would otherwise
      // corrupt the live one.
      const eventSessionId = "sessionId" in event ? event.sessionId : null
      if (
        eventSessionId !== null &&
        eventSessionId !== 0 &&
        device.sessionId !== null &&
        eventSessionId !== device.sessionId
      ) {
        return
      }
      switch (event.event) {
        case "started":
          // May arrive before the start dto resolves — adopt the id so
          // stale events from the previous session can be filtered.
          device.sessionId = event.sessionId
          device.running = true
          device.error = ""
          break
        case "stopped":
        case "cancelled":
          device.running = false
          device.attached = false
          void refreshSessions()
          break
        case "warning":
          device.error = event.message
          break
        case "error":
          device.error = event.message
          device.running = false
          device.attached = false
          void refreshSessions()
          break
        case "version-firmware":
          device.updateStatus = event.updateStatus
          device.firmwareStatus = event.firmwareStatus
          break
        default:
          assertNever(event)
      }
      publish(profileId)
    },
    [publish, refreshSessions],
  )

  const selectProfile = useCallback((profile: MikrotikProfile | null) => {
    setSelectedProfile(profile)
    setGlobalError("")
  }, [])

  const start = useCallback(
    async (profileId?: number) => {
      const targetId = profileId ?? selectedProfile?.id
      if (targetId === undefined) {
        setGlobalError("Select a MikroTik profile before starting monitoring")
        return
      }
      const existing = deviceRefs.current.get(targetId)
      if (existing !== undefined && existing.sessionId === null) {
        // A start for this profile is already in flight — invoking again
        // would overwrite the placeholder and orphan the first session.
        return
      }
      // The mutable entry exists BEFORE the invoke resolves: channel events
      // can land before the promise settles.
      const device = emptyDevice()
      deviceRefs.current.set(targetId, device)
      flushRefs.current.set(targetId, () => flush(targetId))
      try {
        const handleEvent = (event: MikrotikSnapshotEvent) => {
          device.pending.push(event)
          if (!device.flushing) {
            device.flushing = true
            requestAnimationFrame(() => flushRefs.current.get(targetId)?.())
          }
        }
        const dto = await mikrotikStart(targetId, handleEvent, (event) => handleStatus(targetId, event))
        device.sessionId = dto.sessionId
        // A stale terminal event from the previous session may have landed
        // while we were starting — a fresh session is running now.
        device.running = true
        device.error = ""
        setHistory(null)
        publish(targetId)
      } catch (err) {
        // Failed start: remove the placeholder so the chip disappears — but
        // only if the entry is still OURS; a restart may have replaced it.
        // The error itself is surfaced through the global banner.
        if (deviceRefs.current.get(targetId) === device) {
          deviceRefs.current.delete(targetId)
          flushRefs.current.delete(targetId)
          publish(targetId)
        }
        setGlobalError(errorMessage(err))
      }
    },
    [flush, handleStatus, publish, selectedProfile],
  )

  const stop = useCallback(
    async (profileId?: number) => {
      const targetId = profileId ?? selectedProfile?.id
      if (targetId === undefined) return
      const device = deviceRefs.current.get(targetId)
      if (device === undefined || device.sessionId === null) return
      const sessionId = device.sessionId
      try {
        await mikrotikStop(sessionId)
      } catch {
        // The session may already have terminated on its own (terminal Error
        // status event); local cleanup below still applies.
      }
      deviceRefs.current.delete(targetId)
      flushRefs.current.delete(targetId)
      publish(targetId)
      void refreshSessions()
    },
    [publish, refreshSessions, selectedProfile],
  )

  /** Stop + start one device: reattaches a live-but-detached session after a
      view reload so its channels point at this hook instance again. */
  const reconnect = useCallback(
    async (profileId: number) => {
      await stop(profileId)
      await start(profileId)
    },
    [start, stop],
  )

  const loadSession = useCallback(async (id: number) => {
    try {
      const loaded = await mikrotikLoadSession(id)
      const built = buildMikrotikHistory(loaded.snapshots, loaded.session)
      setHistory({
        loadedSession: loaded,
        latestSnapshot: built.latestSnapshot,
        snapshotHistory: built.snapshotHistory,
        rateSeries: built.rateSeries,
        vlans: built.vlans,
        bridgeVlans: built.bridgeVlans,
        sensors: built.sensors,
        updateStatus: built.updateStatus,
        firmwareStatus: built.firmwareStatus,
      })
      setGlobalError("")
    } catch (err) {
      setGlobalError(errorMessage(err))
    }
  }, [])

  const deleteSession = useCallback(
    async (id: number) => {
      try {
        await mikrotikDeleteSession(id)
        if (history?.loadedSession.session.id === id) {
          setHistory(null)
        }
        await refreshSessions()
      } catch (err) {
        setGlobalError(errorMessage(err))
      }
    },
    [history, refreshSessions],
  )

  const deviceFor = useCallback(
    (profile: MikrotikProfile | null): MikrotikDeviceState | null =>
      profile === null ? null : (devices.get(profile.id) ?? null),
    [devices],
  )

  const selectedDevice = deviceFor(selectedProfile)
  // Live data wins over a loaded history view for the same selection.
  const view = selectedDevice !== null && (selectedDevice.running || selectedDevice.latestSnapshot !== null) ? selectedDevice : history

  const checkUpdates = useCallback(async () => {
    if (selectedProfile === null) return null
    try {
      const result = await mikrotikCheckUpdates(selectedProfile.id)
      let device = deviceRefs.current.get(selectedProfile.id)
      if (device === undefined) {
        device = emptyDevice()
        device.running = false
        device.attached = false
        deviceRefs.current.set(selectedProfile.id, device)
      }
      device.updateStatus = result.updateStatus
      device.firmwareStatus = result.firmwareStatus
      publish(selectedProfile.id)
      setGlobalError("")
      return result
    } catch (err) {
      setGlobalError(errorMessage(err))
      return null
    }
  }, [publish, selectedProfile])

  const fetchChangelog = useCallback(async (version: string): Promise<MikrotikChangelogDto | null> => {
    try {
      setGlobalError("")
      return await mikrotikFetchChangelog(version)
    } catch (err) {
      setGlobalError(errorMessage(err))
      return null
    }
  }, [])

  const runBackup = useCallback(async (request: RunBackupRequest): Promise<BackupResultDto | null> => {
    try {
      setGlobalError("")
      return await mikrotikBackup(request.profileId, request.destinationDir, request.backupName, request.password, request.includeRsc, request.overwrite)
    } catch (err) {
      setGlobalError(errorMessage(err))
      return null
    }
  }, [])

  const activeDevices = [...devices.values()].filter((device) => device.running)

  return {
    profiles,
    selectedProfile,
    selectProfile,
    devices,
    activeDevices,
    deviceFor,
    selectedDevice,
    attached: selectedDevice?.attached ?? false,
    running: activeDevices.length > 0,
    latestSnapshot: view?.latestSnapshot ?? null,
    snapshotHistory: view?.snapshotHistory ?? [],
    rateSeries: view?.rateSeries ?? {},
    vlans: view?.vlans ?? [],
    bridgeVlans: view?.bridgeVlans ?? [],
    sensors: view?.sensors ?? [],
    updateStatus: view?.updateStatus ?? null,
    firmwareStatus: view?.firmwareStatus ?? null,
    loadedSession: history?.loadedSession ?? null,
    sessions,
    error: selectedDevice?.error || globalError,
    start,
    stop,
    reconnect,
    loadSession,
    deleteSession,
    refreshProfiles,
    checkUpdates,
    fetchChangelog,
    runBackup,
  }
}
