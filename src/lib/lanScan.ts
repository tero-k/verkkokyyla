import type { InterfaceDto, ScanEvent, ScanHostDto } from "./types"

export function getPrimaryInterface(
  interfaces: readonly InterfaceDto[],
): InterfaceDto | null {
  if (interfaces.length === 0) return null
  const primary = interfaces.find((iface) => iface.isPrimary)
  return primary ?? interfaces[0]
}

export function buildDefaultCidr(iface: {
  readonly ipv4: string
  readonly prefixLen?: number
}): string {
  const prefix = iface.prefixLen ?? 24
  return `${iface.ipv4}/${prefix}`
}

export function applyHostEvents(
  existing: readonly ScanHostDto[],
  events: readonly ScanEvent[],
  maxRows = 500,
): readonly ScanHostDto[] {
  const map = new Map(existing.map((host) => [host.ip, host]))
  for (const event of events) {
    if (event.event === "host") {
      map.set(event.ip, {
        ip: event.ip,
        mac: event.mac,
        vendor: event.vendor,
        hostname: event.hostname,
        foundBy: event.foundBy,
        at: event.at,
        openPorts: event.openPorts,
      })
    }
  }
  return Array.from(map.values()).slice(-maxRows)
}
