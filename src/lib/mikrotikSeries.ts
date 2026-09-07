import type {
  MikrotikBridgeVlanDto,
  MikrotikFirmwareStatus,
  MikrotikInterfaceDto,
  MikrotikLoadedSessionDto,
  MikrotikSensorDto,
  MikrotikSnapshotDto,
  MikrotikSnapshotEvent,
  MikrotikUpdateStatus,
  MikrotikVlanDto,
} from "./types"

export const MIKROTIK_SERIES_CAPACITY = 720

export type MikrotikRatePoint = {
  readonly at: string
  readonly rxBitsPerSecond: number | null
  readonly txBitsPerSecond: number | null
}

export type MikrotikRateSeries = Record<string, readonly MikrotikRatePoint[]>

export type MikrotikHistoryState = {
  readonly latestSnapshot: MikrotikSnapshotEvent | null
  readonly snapshotHistory: readonly MikrotikSnapshotEvent[]
  readonly rateSeries: MikrotikRateSeries
  readonly vlans: readonly MikrotikVlanDto[]
  readonly bridgeVlans: readonly MikrotikBridgeVlanDto[]
  readonly sensors: readonly MikrotikSensorDto[]
  readonly updateStatus: MikrotikUpdateStatus | null
  readonly firmwareStatus: MikrotikFirmwareStatus | null
}

type InterfaceCounters = {
  readonly atMs: number
  readonly rxByte: number | null
  readonly txByte: number | null
}

type MutableRateSeries = Record<string, MikrotikRatePoint[]>

function parseJsonArray<T>(json: string | null): readonly T[] {
  if (json === null) return []
  const parsed: unknown = JSON.parse(json)
  return Array.isArray(parsed) ? parsed : []
}

function parseJsonObject<T>(json: string | null): T | null {
  if (json === null) return null
  const parsed: unknown = JSON.parse(json)
  return typeof parsed === "object" && parsed !== null ? (parsed as T) : null
}

export function pushMikrotikRatePoint(
  series: MikrotikRateSeries,
  snapshot: MikrotikSnapshotEvent,
): MikrotikRateSeries {
  const next: MutableRateSeries = {}
  for (const [name, points] of Object.entries(series)) {
    next[name] = [...points]
  }
  for (const iface of snapshot.interfaces) {
    const points = next[iface.name] ?? []
    points.push({
      at: snapshot.at,
      rxBitsPerSecond: iface.rxBitsPerSecond,
      txBitsPerSecond: iface.txBitsPerSecond,
    })
    next[iface.name] = points.slice(-MIKROTIK_SERIES_CAPACITY)
  }
  return next
}

export function buildMikrotikSnapshot(row: MikrotikSnapshotDto): MikrotikSnapshotEvent {
  const sensors = parseJsonArray<MikrotikSensorDto>(row.sensorsJson)
  return {
    event: "snapshot",
    sessionId: row.sessionId,
    at: row.at,
    resources: {
      cpuLoad: row.cpuLoad,
      memUsedBytes: row.memUsedBytes,
      memTotalBytes: row.memTotalBytes,
      uptime: row.uptime,
      boardName: row.boardName,
      routerosVersion: row.routerosVersion,
      architectureName: row.architectureName,
    },
    sensors: row.sensorsJson === null ? null : sensors,
    sensorsSupported: row.sensorsJson !== null,
    interfaces: parseJsonArray<MikrotikInterfaceDto>(row.interfacesJson),
    vlans: row.vlansJson === null ? null : parseJsonArray<MikrotikVlanDto>(row.vlansJson),
    bridgeVlans:
      row.bridgeVlansJson === null
        ? null
        : parseJsonArray<MikrotikBridgeVlanDto>(row.bridgeVlansJson),
    warning: row.warning,
  }
}

function elapsedSeconds(previous: InterfaceCounters, atMs: number): number | null {
  const elapsed = (atMs - previous.atMs) / 1_000
  return elapsed > 0 ? elapsed : null
}

function reconstructedRate(
  previous: number | null,
  current: number | null,
  elapsed: number | null,
): number | null {
  if (previous === null || current === null || elapsed === null) return null
  if (current < previous) return null
  return (8 * (current - previous)) / elapsed
}

function withReconstructedRates(
  snapshot: MikrotikSnapshotEvent,
  previous: ReadonlyMap<string, InterfaceCounters>,
): MikrotikSnapshotEvent {
  return {
    ...snapshot,
    interfaces: snapshot.interfaces.map((iface) => {
      const counters = previous.get(iface.name)
      const atMs = Date.parse(snapshot.at)
      const elapsed = counters === undefined ? null : elapsedSeconds(counters, atMs)
      return {
        ...iface,
        rxBitsPerSecond:
          counters === undefined
            ? null
            : reconstructedRate(counters.rxByte, iface.rxByte, elapsed),
        txBitsPerSecond:
          counters === undefined
            ? null
            : reconstructedRate(counters.txByte, iface.txByte, elapsed),
      }
    }),
  }
}

function rememberCounters(
  snapshot: MikrotikSnapshotEvent,
): ReadonlyMap<string, InterfaceCounters> {
  const next = new Map<string, InterfaceCounters>()
  const atMs = Date.parse(snapshot.at)
  for (const iface of snapshot.interfaces) {
    next.set(iface.name, {
      atMs,
      rxByte: iface.rxByte,
      txByte: iface.txByte,
    })
  }
  return next
}

export function buildMikrotikHistory(
  snapshots: readonly MikrotikSnapshotDto[],
  session?: Pick<MikrotikLoadedSessionDto["session"], "updateStatusJson" | "firmwareStatusJson">,
): MikrotikHistoryState {
  let latestSnapshot: MikrotikSnapshotEvent | null = null
  let snapshotHistory: readonly MikrotikSnapshotEvent[] = []
  let rateSeries: MikrotikRateSeries = {}
  let vlans: readonly MikrotikVlanDto[] = []
  let bridgeVlans: readonly MikrotikBridgeVlanDto[] = []
  let sensors: readonly MikrotikSensorDto[] = []
  let previousCounters: ReadonlyMap<string, InterfaceCounters> = new Map()

  for (const row of snapshots) {
    const parsed = buildMikrotikSnapshot(row)
    const rebuilt = withReconstructedRates(parsed, previousCounters)
    previousCounters = rememberCounters(parsed)
    latestSnapshot = rebuilt
    snapshotHistory = [...snapshotHistory, rebuilt]
    rateSeries = pushMikrotikRatePoint(rateSeries, rebuilt)
    if (rebuilt.vlans !== null) vlans = rebuilt.vlans
    if (rebuilt.bridgeVlans !== null) bridgeVlans = rebuilt.bridgeVlans
    if (rebuilt.sensors !== null) sensors = rebuilt.sensors
  }

  return {
    latestSnapshot,
    snapshotHistory,
    rateSeries,
    vlans,
    bridgeVlans,
    sensors,
    updateStatus: parseJsonObject<MikrotikUpdateStatus>(session?.updateStatusJson ?? null),
    firmwareStatus: parseJsonObject<MikrotikFirmwareStatus>(session?.firmwareStatusJson ?? null),
  }
}
