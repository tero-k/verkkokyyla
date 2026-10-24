import type {
  OpnsenseSnapshotDto,
  OpnsenseSystemSectionDto,
  OpnsenseVpnSectionDto,
} from "./types"

export const OPNSENSE_TRAFFIC_CAPACITY = 720

export type OpnsenseTrafficSource =
  | { readonly kind: "interface"; readonly name: string }
  | { readonly kind: "wireguard"; readonly interfaceName: string; readonly name: string }
  | { readonly kind: "openvpn"; readonly name: string }

export type OpnsenseTrafficHistory = {
  readonly selected: readonly OpnsenseTrafficSource[]
  readonly times: readonly number[]
  readonly rates: Readonly<
    Record<string, { readonly rx: readonly (number | null)[]; readonly tx: readonly (number | null)[] }>
  >
  readonly baselines: Readonly<
    Record<string, { readonly atMs: number; readonly rxBytes: number | null; readonly txBytes: number | null }>
  >
}

type Counters = { readonly rxBytes: number | null; readonly txBytes: number | null }

export function trafficSourceKey(source: OpnsenseTrafficSource): string {
  return source.kind === "wireguard"
    ? JSON.stringify([source.kind, source.interfaceName, source.name])
    : JSON.stringify([source.kind, source.name])
}

export function createOpnsenseTrafficHistory(): OpnsenseTrafficHistory {
  return { selected: [], times: [], rates: {}, baselines: {} }
}

function validCounter(value: number | null | undefined): number | null {
  return value !== null && value !== undefined && Number.isSafeInteger(value) && value >= 0
    ? value
    : null
}

export function opnsenseTrafficCounters(
  source: OpnsenseTrafficSource,
  system: OpnsenseSystemSectionDto | null,
  vpn: OpnsenseVpnSectionDto | null,
): Counters | null {
  if (source.kind === "interface") {
    const iface = system?.interfaces.find((candidate) => candidate.name === source.name)
    return iface === undefined
      ? null
      : { rxBytes: validCounter(iface.rxBytes), txBytes: validCounter(iface.txBytes) }
  }

  if (source.kind === "wireguard") {
    const service = vpn?.wireguard
    if (service?.state !== "ok" || service.data === null || service.data === undefined) return null
    const peer = service.data.interfaces
      .find((candidate) => candidate.name === source.interfaceName)
      ?.peers.find((candidate) => candidate.name === source.name)
    return peer === undefined
      ? null
      : { rxBytes: validCounter(peer.transferRxBytes), txBytes: validCounter(peer.transferTxBytes) }
  }

  const service = vpn?.openvpn
  if (service?.state !== "ok" || service.data === null || service.data === undefined) return null
  const session = service.data.find((candidate) => candidate.name === source.name)
  return session === undefined
    ? null
    : { rxBytes: validCounter(session.bytesReceived), txBytes: validCounter(session.bytesSent) }
}

export function toggleOpnsenseTrafficSource(
  history: OpnsenseTrafficHistory,
  source: OpnsenseTrafficSource,
  baseline: { at: string; rxBytes: number | null; txBytes: number | null } | null,
): OpnsenseTrafficHistory {
  const key = trafficSourceKey(source)
  const selectedIndex = history.selected.findIndex((candidate) => trafficSourceKey(candidate) === key)
  if (selectedIndex >= 0) {
    const rates = { ...history.rates }
    const baselines = { ...history.baselines }
    delete rates[key]
    delete baselines[key]
    const selected = history.selected.filter((_, index) => index !== selectedIndex)
    return selected.length === 0
      ? createOpnsenseTrafficHistory()
      : { ...history, selected, rates, baselines }
  }

  const atMs = baseline === null ? Number.NaN : Date.parse(baseline.at)
  const baselines = { ...history.baselines }
  if (Number.isFinite(atMs)) {
    baselines[key] = {
      atMs,
      rxBytes: validCounter(baseline?.rxBytes),
      txBytes: validCounter(baseline?.txBytes),
    }
  }
  return {
    ...history,
    selected: [...history.selected, source],
    rates: {
      ...history.rates,
      [key]: {
        rx: Array<number | null>(history.times.length).fill(null),
        tx: Array<number | null>(history.times.length).fill(null),
      },
    },
    baselines,
  }
}

function rate(previous: number | null, current: number | null, elapsedSeconds: number): number | null {
  if (previous === null || current === null || current < previous) return null
  return (8 * (current - previous)) / elapsedSeconds
}

export function appendOpnsenseTrafficSnapshot(
  history: OpnsenseTrafficHistory,
  snapshot: OpnsenseSnapshotDto,
): OpnsenseTrafficHistory {
  if (history.selected.length === 0) return history
  const atMs = Date.parse(snapshot.at)
  const previousAt = history.times.at(-1)
  if (!Number.isFinite(atMs) || (previousAt !== undefined && atMs <= previousAt * 1000)) return history

  const system = snapshot.system.ok && snapshot.system.data != null ? snapshot.system.data : null
  const vpn = snapshot.vpn.ok && snapshot.vpn.data != null ? snapshot.vpn.data : null
  const rates: Record<string, { rx: (number | null)[]; tx: (number | null)[] }> = {}
  const baselines: Record<string, { atMs: number; rxBytes: number | null; txBytes: number | null }> = {}

  for (const source of history.selected) {
    const key = trafficSourceKey(source)
    const previous = history.baselines[key]
    const current = opnsenseTrafficCounters(source, system, vpn)
    const elapsedSeconds = previous === undefined ? null : (atMs - previous.atMs) / 1000
    const canCalculate = elapsedSeconds !== null && elapsedSeconds > 0
    const rxBytes = current?.rxBytes ?? null
    const txBytes = current?.txBytes ?? null
    rates[key] = {
      rx: [
        ...history.rates[key].rx,
        canCalculate ? rate(previous?.rxBytes ?? null, rxBytes, elapsedSeconds) : null,
      ],
      tx: [
        ...history.rates[key].tx,
        canCalculate ? rate(previous?.txBytes ?? null, txBytes, elapsedSeconds) : null,
      ],
    }
    baselines[key] = { atMs, rxBytes, txBytes }
  }

  const times = [...history.times, atMs / 1000]
  const overflow = Math.max(0, times.length - OPNSENSE_TRAFFIC_CAPACITY)
  return {
    selected: history.selected,
    times: overflow === 0 ? times : times.slice(overflow),
    rates: Object.fromEntries(
      Object.entries(rates).map(([key, series]) => [
        key,
        {
          rx: overflow === 0 ? series.rx : series.rx.slice(overflow),
          tx: overflow === 0 ? series.tx : series.tx.slice(overflow),
        },
      ]),
    ),
    baselines,
  }
}
