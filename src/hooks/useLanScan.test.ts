import { describe, expect, it } from "vitest"
import {
  applyHostEvents,
  buildDefaultCidr,
  getPrimaryInterface,
} from "../lib/lanScan"
import type { InterfaceDto, ScanEvent, ScanHostDto } from "../lib/types"

function makeInterface(
  name: string,
  ipv4: string,
  prefixLen: number,
  isPrimary: boolean,
): InterfaceDto {
  return {
    name,
    description: "test",
    ipv4,
    prefixLen,
    isPrimary,
  }
}

function makeHostEvent(
  ip: string,
  overrides: Partial<Omit<ScanEvent, "event" | "ip">> = {},
): ScanEvent {
  return {
    event: "host",
    ip,
    mac: overrides.mac ?? null,
    vendor: overrides.vendor ?? null,
    hostname: overrides.hostname ?? null,
    foundBy: overrides.foundBy ?? "ping",
    at: overrides.at ?? "2026-08-19T12:00:00Z",
    openPorts: overrides.openPorts ?? [],
  }
}

describe("getPrimaryInterface", () => {
  it("selects the primary interface when one exists", () => {
    const interfaces: readonly InterfaceDto[] = [
      makeInterface("eth0", "192.168.1.10", 24, false),
      makeInterface("wlan0", "192.168.2.10", 24, true),
    ]
    const selected = getPrimaryInterface(interfaces)
    expect(selected).toEqual(interfaces[1])
  })

  it("falls back to the first interface when no primary is marked", () => {
    const interfaces: readonly InterfaceDto[] = [
      makeInterface("eth0", "10.0.0.5", 24, false),
      makeInterface("wlan0", "10.0.1.5", 24, false),
    ]
    const selected = getPrimaryInterface(interfaces)
    expect(selected).toEqual(interfaces[0])
  })

  it("returns null for an empty interface list", () => {
    expect(getPrimaryInterface([])).toBeNull()
  })
})

describe("buildDefaultCidr", () => {
  it("builds a CIDR from the interface IPv4 and prefix length", () => {
    const iface = makeInterface("eth0", "192.168.1.10", 24, true)
    expect(buildDefaultCidr(iface)).toBe("192.168.1.10/24")
  })

  it("falls back to /24 when prefix length is missing", () => {
    const iface = { ipv4: "192.168.1.10" }
    expect(buildDefaultCidr(iface)).toBe("192.168.1.10/24")
  })
})

describe("applyHostEvents", () => {
  it("appends new hosts", () => {
    const existing: readonly ScanHostDto[] = []
    const events: readonly ScanEvent[] = [
      makeHostEvent("192.168.1.1", { mac: "00:11:22:33:44:55", foundBy: "arp" }),
      makeHostEvent("192.168.1.2", { hostname: "second" }),
    ]
    const rows = applyHostEvents(existing, events)
    expect(rows).toHaveLength(2)
    expect(rows[0].ip).toBe("192.168.1.1")
    expect(rows[1].hostname).toBe("second")
  })

  it("replaces an existing host by IP and keeps the latest version", () => {
    const existing: readonly ScanHostDto[] = [
      {
        ip: "192.168.1.1",
        mac: "00:11:22:33:44:55",
        vendor: null,
        hostname: null,
        foundBy: "ping",
        at: "2026-08-19T12:00:00Z",
        openPorts: [],
      },
    ]
    const events: readonly ScanEvent[] = [
      makeHostEvent("192.168.1.1", { mac: "AA:BB:CC:DD:EE:FF", vendor: "Acme", foundBy: "arp" }),
    ]
    const rows = applyHostEvents(existing, events)
    expect(rows).toHaveLength(1)
    expect(rows[0].mac).toBe("AA:BB:CC:DD:EE:FF")
    expect(rows[0].vendor).toBe("Acme")
    expect(rows[0].foundBy).toBe("arp")
  })

  it("caps the visible rows at 500", () => {
    const existing: readonly ScanHostDto[] = []
    const events: readonly ScanEvent[] = Array.from({ length: 600 }, (_, index) =>
      makeHostEvent(`192.168.1.${index + 1}`),
    )
    const rows = applyHostEvents(existing, events)
    expect(rows).toHaveLength(500)
    expect(rows[0].ip).toBe("192.168.1.101")
    expect(rows[rows.length - 1].ip).toBe("192.168.1.600")
  })
})
