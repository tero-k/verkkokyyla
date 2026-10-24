import { useCallback, useEffect, useRef, useState } from "react"
import {
  opnsenseBackup,
  opnsenseCheckUpdates,
  opnsenseCreateProfile,
  opnsenseDeleteProfile,
  opnsenseListActive,
  opnsenseListProfiles,
  opnsenseListRevisions,
  opnsenseSetCredentials,
  opnsenseStart,
  opnsenseStop,
  opnsenseTestConnection,
  opnsenseUpdateProfile,
} from "../lib/ipc"
import {
  opnsenseErrorPayload,
  type CreateOpnsenseProfileRequest,
  type OpnsenseActiveMonitorDto,
  type OpnsenseBackupResultDto,
  type OpnsenseDeleteResultDto,
  type OpnsenseVersionCheckDto,
  type OpnsenseProfile,
  type OpnsenseRevisionDto,
  type OpnsenseRulesSectionDto,
  type OpnsenseSectionDto,
  type OpnsenseSectionErrorDto,
  type OpnsenseSnapshotDto,
  type OpnsenseStatusEvent,
  type OpnsenseSystemSectionDto,
  type OpnsenseTestConnectionDto,
  type OpnsenseVpnSectionDto,
  type UpdateOpnsenseProfileRequest,
} from "../lib/types"
import {
  appendOpnsenseTrafficSnapshot,
  createOpnsenseTrafficHistory,
  opnsenseTrafficCounters,
  toggleOpnsenseTrafficSource,
  type OpnsenseTrafficHistory,
  type OpnsenseTrafficSource,
} from "../lib/opnsenseTraffic"

export type RunOpnsenseBackupRequest = {
  readonly profileId: number
  readonly destinationDir: string
  readonly backupName: string
  readonly passphrase: string
  /** Saved revision to download; null = newest saved revision. */
  readonly revisionId: string | null
}

/** Last-good view of one snapshot section: fresh data, or the previous
    data kept and marked stale after a failed refresh. */
export type OpnsenseSectionView<T> = {
  readonly data: T | null
  readonly stale: boolean
  readonly error: OpnsenseSectionErrorDto | null
  /** Timestamp of the last successful fetch of this section. */
  readonly updatedAt: string | null
}

/** Live state of ONE monitored firewall. `attached` distinguishes monitors
    whose event channels this hook instance holds: after a view reload the
    backend monitor keeps running but its channel points at the dead hook,
    so the device shows as live-but-detached until the user reconnects. */
export type OpnsenseDeviceState = {
  readonly profileId: number
  readonly running: boolean
  readonly attached: boolean
  readonly rules: OpnsenseSectionView<OpnsenseRulesSectionDto>
  readonly system: OpnsenseSectionView<OpnsenseSystemSectionDto>
  readonly vpn: OpnsenseSectionView<OpnsenseVpnSectionDto>
  readonly traffic: OpnsenseTrafficHistory
  /** Lifecycle failure reported on the status channel. */
  readonly statusError: string
}

type MutableDevice = {
  running: boolean
  attached: boolean
  rules: OpnsenseSectionView<OpnsenseRulesSectionDto>
  system: OpnsenseSectionView<OpnsenseSystemSectionDto>
  vpn: OpnsenseSectionView<OpnsenseVpnSectionDto>
  traffic: OpnsenseTrafficHistory
  statusError: string
  pending: OpnsenseSnapshotDto[]
  flushing: boolean
}

function emptySection<T>(): OpnsenseSectionView<T> {
  return { data: null, stale: false, error: null, updatedAt: null }
}

function emptyDevice(): MutableDevice {
  return {
    running: true,
    attached: true,
    rules: emptySection(),
    system: emptySection(),
    vpn: emptySection(),
    traffic: createOpnsenseTrafficHistory(),
    statusError: "",
    pending: [],
    flushing: false,
  }
}

function mergeSection<T>(
  current: OpnsenseSectionView<T>,
  section: OpnsenseSectionDto<T>,
  at: string,
): OpnsenseSectionView<T> {
  if (section.ok && section.data !== null && section.data !== undefined) {
    return { data: section.data, stale: false, error: null, updatedAt: at }
  }
  // Failed refresh: keep the last good data and mark it stale — never
  // display zeroed/empty data as if it were fresh.
  return {
    data: current.data,
    stale: current.data !== null,
    error: section.error ?? null,
    updatedAt: current.updatedAt,
  }
}

function deviceStateOf(profileId: number, device: MutableDevice): OpnsenseDeviceState {
  return {
    profileId,
    running: device.running,
    attached: device.attached,
    rules: device.rules,
    system: device.system,
    vpn: device.vpn,
    traffic: device.traffic,
    statusError: device.statusError,
  }
}

function errorMessage(err: unknown): string {
  return opnsenseErrorPayload(err).message
}

export function useOpnsense() {
  const [profiles, setProfiles] = useState<readonly OpnsenseProfile[]>([])
  const [selectedProfile, setSelectedProfile] = useState<OpnsenseProfile | null>(null)
  const [devices, setDevices] = useState<ReadonlyMap<number, OpnsenseDeviceState>>(new Map())
  const [globalError, setGlobalError] = useState("")
  /** Firmware version check for the profile that requested it. */
  const [firmware, setFirmware] = useState<{
    readonly profileId: number
    readonly check: OpnsenseVersionCheckDto
  } | null>(null)
  /** Saved config revisions for the profile that requested them. */
  const [revisions, setRevisions] = useState<{
    readonly profileId: number
    readonly items: readonly OpnsenseRevisionDto[]
  } | null>(null)

  const deviceRefs = useRef(new Map<number, MutableDevice>())
  const flushRefs = useRef(new Map<number, () => void>())
  const publishRef = useRef<(profileId: number) => void>(() => {})

  const publish = useCallback((profileId: number) => {
    setDevices((prev) => {
      const next = new Map(prev)
      const device = deviceRefs.current.get(profileId)
      if (
        device === undefined ||
        (!device.running && device.rules.data === null && device.system.data === null &&
          device.vpn.data === null && device.statusError === "")
      ) {
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
      const nextProfiles = await opnsenseListProfiles()
      setProfiles(nextProfiles)
      setSelectedProfile((current) =>
        nextProfiles.find((profile) => profile.id === current?.id) ?? nextProfiles[0] ?? null,
      )
    } catch (err) {
      setGlobalError(errorMessage(err))
    }
  }, [])

  useEffect(() => {
    void refreshProfiles()
  }, [refreshProfiles])

  // Rehydrate the device strip after a (re)mount: monitors that are
  // already live show up as running-but-detached — the channels from the
  // previous hook instance are gone, so the view offers a reconnect.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const active: OpnsenseActiveMonitorDto[] = await opnsenseListActive()
        if (cancelled || active.length === 0) return
        for (const entry of active) {
          if (!deviceRefs.current.has(entry.profileId)) {
            const device = emptyDevice()
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

  const applySnapshot = useCallback((device: MutableDevice, snapshot: OpnsenseSnapshotDto) => {
    device.traffic = appendOpnsenseTrafficSnapshot(device.traffic, snapshot)
    if (snapshot.rules !== undefined) device.rules = mergeSection(device.rules, snapshot.rules, snapshot.at)
    device.system = mergeSection(device.system, snapshot.system, snapshot.at)
    device.vpn = mergeSection(device.vpn, snapshot.vpn, snapshot.at)
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
    (profileId: number, event: OpnsenseStatusEvent) => {
      const device = deviceRefs.current.get(profileId)
      if (device === undefined) return
      switch (event.event) {
        case "started":
          device.running = true
          device.statusError = ""
          break
        case "stopped":
          device.running = false
          device.attached = false
          break
        case "warning":
          device.statusError = event.message
          break
        case "error":
          device.statusError = event.message
          device.running = false
          device.attached = false
          break
      }
      publish(profileId)
    },
    [publish],
  )

  const toggleTrafficSource = useCallback(
    (profileId: number, source: OpnsenseTrafficSource) => {
      const device = deviceRefs.current.get(profileId)
      if (device === undefined) return
      const freshSystem = !device.system.stale && device.system.updatedAt !== null
        ? device.system.data
        : null
      const freshVpn = !device.vpn.stale && device.vpn.updatedAt !== null ? device.vpn.data : null
      const counters = opnsenseTrafficCounters(source, freshSystem, freshVpn)
      const updatedAt = source.kind === "interface" ? device.system.updatedAt : device.vpn.updatedAt
      device.traffic = toggleOpnsenseTrafficSource(device.traffic, source,
        counters === null || updatedAt === null ? null : { at: updatedAt, ...counters })
      publish(profileId)
    },
    [publish],
  )

  const selectProfile = useCallback((profile: OpnsenseProfile | null) => {
    setSelectedProfile(profile)
    setGlobalError("")
  }, [])

  const start = useCallback(
    async (profileId?: number) => {
      const targetId = profileId ?? selectedProfile?.id
      if (targetId === undefined) {
        setGlobalError("Select an OPNsense profile before starting monitoring")
        return
      }
      const existing = deviceRefs.current.get(targetId)
      if (existing?.running && existing.attached) return
      // The mutable entry exists BEFORE the invoke resolves: channel events
      // can land before the promise settles.
      const device = emptyDevice()
      deviceRefs.current.set(targetId, device)
      flushRefs.current.set(targetId, () => flush(targetId))
      try {
        const handleSnapshot = (snapshot: OpnsenseSnapshotDto) => {
          if (deviceRefs.current.get(targetId) !== device) return
          device.pending.push(snapshot)
          if (!device.flushing) {
            device.flushing = true
            requestAnimationFrame(() => flushRefs.current.get(targetId)?.())
          }
        }
        await opnsenseStart(targetId, handleSnapshot, (event) => {
          if (deviceRefs.current.get(targetId) === device) handleStatus(targetId, event)
        })
        if (deviceRefs.current.get(targetId) !== device) return
        device.running = true
        device.statusError = ""
        publish(targetId)
      } catch (err) {
        // Failed start: remove the placeholder so the chip disappears — but
        // only if the entry is still OURS; a restart may have replaced it.
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
      if (device === undefined) return
      try {
        await opnsenseStop(targetId)
      } catch {
        // The monitor may already have terminated on its own (terminal Error
        // status event); local cleanup below still applies.
      }
      if (deviceRefs.current.get(targetId) === device) {
        deviceRefs.current.delete(targetId)
        flushRefs.current.delete(targetId)
        publish(targetId)
      }
    },
    [publish, selectedProfile],
  )

  /** Stop + start one device: reattaches a live-but-detached monitor after a
      view reload so its channels point at this hook instance again. */
  const reconnect = useCallback(
    async (profileId: number) => {
      await stop(profileId)
      await start(profileId)
    },
    [start, stop],
  )

  const createProfile = useCallback(
    async (request: CreateOpnsenseProfileRequest): Promise<OpnsenseProfile | null> => {
      try {
        const profile = await opnsenseCreateProfile(request)
        setGlobalError("")
        await refreshProfiles()
        return profile
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [refreshProfiles],
  )

  const updateProfile = useCallback(
    async (request: UpdateOpnsenseProfileRequest): Promise<OpnsenseProfile | null> => {
      try {
        const profile = await opnsenseUpdateProfile(request)
        setGlobalError("")
        await refreshProfiles()
        return profile
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [refreshProfiles],
  )

  const deleteProfile = useCallback(
    async (id: number): Promise<OpnsenseDeleteResultDto | null> => {
      try {
        const result = await opnsenseDeleteProfile(id)
        setGlobalError(result.warning ?? "")
        // Drop any device state and firmware/revision caches for the id.
        deviceRefs.current.delete(id)
        flushRefs.current.delete(id)
        publish(id)
        setFirmware((current) => (current?.profileId === id ? null : current))
        setRevisions((current) => (current?.profileId === id ? null : current))
        await refreshProfiles()
        return result
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [publish, refreshProfiles],
  )

  const setCredentials = useCallback(async (id: number, key: string, secret: string) => {
    try {
      await opnsenseSetCredentials(id, key, secret)
      setGlobalError("")
      await refreshProfiles()
      return true
    } catch (err) {
      setGlobalError(errorMessage(err))
      return false
    }
  }, [refreshProfiles])

  const testConnection = useCallback(
    async (id: number): Promise<OpnsenseTestConnectionDto | null> => {
      try {
        const result = await opnsenseTestConnection(id)
        setGlobalError("")
        return result
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [],
  )

  /** Compares the installed core version against the official OPNsense
      release CDN and returns the full check result. */
  const checkFirmwareUpdates = useCallback(
    async (profileId?: number): Promise<OpnsenseVersionCheckDto | null> => {
      const targetId = profileId ?? selectedProfile?.id
      if (targetId === undefined) return null
      try {
        const check = await opnsenseCheckUpdates(targetId)
        setFirmware({ profileId: targetId, check })
        setGlobalError("")
        return check
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [selectedProfile],
  )

  const refreshRevisions = useCallback(
    async (profileId?: number): Promise<readonly OpnsenseRevisionDto[] | null> => {
      const targetId = profileId ?? selectedProfile?.id
      if (targetId === undefined) return null
      try {
        const items = await opnsenseListRevisions(targetId)
        // Newest first by the device's own timestamp.
        const sorted = [...items].sort((a, b) => (b.timeIso ?? "").localeCompare(a.timeIso ?? ""))
        setRevisions({ profileId: targetId, items: sorted })
        setGlobalError("")
        return sorted
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [selectedProfile],
  )

  const runBackup = useCallback(
    async (request: RunOpnsenseBackupRequest): Promise<OpnsenseBackupResultDto | null> => {
      try {
        const result = await opnsenseBackup(
          request.profileId,
          request.destinationDir,
          request.backupName,
          request.passphrase,
          request.revisionId,
        )
        setGlobalError("")
        return result
      } catch (err) {
        setGlobalError(errorMessage(err))
        return null
      }
    },
    [],
  )

  const deviceFor = useCallback(
    (profile: OpnsenseProfile | null): OpnsenseDeviceState | null =>
      profile === null ? null : (devices.get(profile.id) ?? null),
    [devices],
  )

  const selectedDevice = deviceFor(selectedProfile)
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
    error: selectedDevice?.statusError || globalError,
    start,
    stop,
    reconnect,
    toggleTrafficSource,
    refreshProfiles,
    createProfile,
    updateProfile,
    deleteProfile,
    setCredentials,
    testConnection,
    firmware,
    checkFirmwareUpdates,
    revisions,
    refreshRevisions,
    runBackup,
  }
}
