import { describe, expect, it } from "vitest"
import {
  appendOpnsenseTrafficSnapshot,
  createOpnsenseTrafficHistory,
  OPNSENSE_TRAFFIC_CAPACITY,
  opnsenseTrafficCounters,
  toggleOpnsenseTrafficSource,
  trafficSourceKey,
  type OpnsenseTrafficHistory,
  type OpnsenseTrafficSource,
} from "./opnsenseTraffic"
import type {
  OpnsenseInterfaceDto,
  OpnsenseOpenvpnSessionDto,
  OpnsenseSnapshotDto,
  OpnsenseVpnSectionDto,
  OpnsenseWireguardInterfaceDto,
} from "./types"

const firstAt = "2026-10-24T12:00:00Z"
const secondAt = "2026-10-24T12:00:05Z"
const thirdAt = "2026-10-24T12:00:10Z"

function iface(name: string, rxBytes: number | null, txBytes: number | null): OpnsenseInterfaceDto {
  return { name, enabled: true, ipv4: [], ipv6: [], rxBytes, txBytes }
}

function wireguard(interfaces: readonly OpnsenseWireguardInterfaceDto[]): OpnsenseVpnSectionDto["wireguard"] {
  return { state: "ok", data: { interfaces } }
}

function peer(name: string, transferRxBytes: number | null, transferTxBytes: number | null) {
  return { name, status: "active", transferRxBytes, transferTxBytes }
}

function openvpn(sessions: readonly OpnsenseOpenvpnSessionDto[]): OpnsenseVpnSectionDto["openvpn"] {
  return { state: "ok", data: sessions }
}

function session(name: string, bytesReceived: number | null, bytesSent: number | null): OpnsenseOpenvpnSessionDto {
  return { name, kind: "client", connected: true, bytesReceived, bytesSent }
}

function snapshot(
  at: string,
  interfaces: readonly OpnsenseInterfaceDto[] = [iface("igb0", 0, 0)],
  vpn: OpnsenseVpnSectionDto = {
    wireguard: wireguard([{ name: "wg0", peers: [peer("remote office", 0, 0)] }]),
    openvpn: openvpn([session("road-1", 0, 0)]),
    ipsec: { state: "unavailable", data: null },
    tailscale: { state: "unavailable", data: null },
  },
  systemOk = true,
  vpnOk = true,
): OpnsenseSnapshotDto {
  return {
    profileId: 1,
    at,
    system: systemOk
      ? { ok: true, data: { interfaces, interfacesTruncated: false } }
      : { ok: false, error: { kind: "http", message: "failed" } },
    vpn: vpnOk ? { ok: true, data: vpn } : { ok: false, error: { kind: "http", message: "failed" } },
  }
}

function select(
  source: OpnsenseTrafficSource,
  at = firstAt,
  rxBytes: number | null = 0,
  txBytes: number | null = 0,
): OpnsenseTrafficHistory {
  return toggleOpnsenseTrafficSource(createOpnsenseTrafficHistory(), source, { at, rxBytes, txBytes })
}

const igb0: OpnsenseTrafficSource = { kind: "interface", name: "igb0" }
const lagg0: OpnsenseTrafficSource = { kind: "interface", name: "lagg0" }
const wg: OpnsenseTrafficSource = { kind: "wireguard", interfaceName: "wg0", name: "remote office" }
const road: OpnsenseTrafficSource = { kind: "openvpn", name: "road-1" }

describe("OPNsense traffic history", () => {
  it("derives bit/s from elapsed time and pads newly selected interfaces", () => {
    let history = select(igb0)
    history = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt))
    history = appendOpnsenseTrafficSnapshot(history, snapshot(secondAt, [iface("igb0", 1000, 500), iface("lagg0", 5000, 2000)]))
    expect(history.rates[trafficSourceKey(igb0)]).toEqual({ rx: [null, 1600], tx: [null, 800] })
    expect(history.times).toEqual([Date.parse(firstAt) / 1000, Date.parse(secondAt) / 1000])

    history = toggleOpnsenseTrafficSource(history, lagg0, { at: secondAt, rxBytes: 5000, txBytes: 2000 })
    expect(history.rates[trafficSourceKey(lagg0)]).toEqual({ rx: [null, null], tx: [null, null] })
    history = appendOpnsenseTrafficSnapshot(history, snapshot(thirdAt, [iface("igb0", 2000, 1000), iface("lagg0", 6000, 2500)]))
    expect(history.rates[trafficSourceKey(lagg0)]).toEqual({ rx: [null, null, 1600], tx: [null, null, 800] })
  })

  it("calculates WireGuard peer and OpenVPN session rates independently", () => {
    let wgHistory = select(wg)
    wgHistory = appendOpnsenseTrafficSnapshot(wgHistory, snapshot(firstAt))
    wgHistory = appendOpnsenseTrafficSnapshot(wgHistory, snapshot(secondAt, [], {
      wireguard: wireguard([{ name: "wg0", peers: [peer("remote office", 2000, 1000)] }]),
      openvpn: openvpn([]), ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }))
    expect(wgHistory.rates[trafficSourceKey(wg)]).toEqual({ rx: [null, 3200], tx: [null, 1600] })

    let vpnHistory = select(road)
    vpnHistory = appendOpnsenseTrafficSnapshot(vpnHistory, snapshot(firstAt))
    vpnHistory = appendOpnsenseTrafficSnapshot(vpnHistory, snapshot(secondAt, [], {
      wireguard: wireguard([]),
      openvpn: openvpn([session("road-1", 3000, 1500)]),
      ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }))
    expect(vpnHistory.rates[trafficSourceKey(road)]).toEqual({ rx: [null, 4800], tx: [null, 2400] })
  })

  it("keeps a valid direction when the other counter is missing or invalid", () => {
    let history = toggleOpnsenseTrafficSource(createOpnsenseTrafficHistory(), road, {
      at: firstAt, rxBytes: null, txBytes: 100,
    })
    history = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt, [], {
      wireguard: wireguard([]), openvpn: openvpn([session("road-1", null, 100)]),
      ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }))
    history = appendOpnsenseTrafficSnapshot(history, snapshot(secondAt, [], {
      wireguard: wireguard([]), openvpn: openvpn([session("road-1", 500, 600)]),
      ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }))
    expect(history.rates[trafficSourceKey(road)]).toEqual({ rx: [null, null], tx: [null, 800] })
    expect(opnsenseTrafficCounters(igb0, { interfaces: [iface("igb0", -1, Number.MAX_SAFE_INTEGER + 1)], interfacesTruncated: false }, null))
      .toEqual({ rxBytes: null, txBytes: null })
  })

  it("does not turn failed retained sections or unavailable VPN services into stale rates", () => {
    let history = select(road)
    history = toggleOpnsenseTrafficSource(history, igb0, { at: firstAt, rxBytes: 0, txBytes: 0 })
    history = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt))
    history = appendOpnsenseTrafficSnapshot(history, snapshot(secondAt, [iface("igb0", 1000, 1000)], {
      wireguard: { state: "access-denied" },
      openvpn: openvpn([session("road-1", 1000, 500)]),
      ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }, false))
    expect(history.rates[trafficSourceKey(igb0)]).toEqual({ rx: [null, null], tx: [null, null] })
    expect(history.rates[trafficSourceKey(road)]).toEqual({ rx: [null, 1600], tx: [null, 800] })
    expect(history.baselines[trafficSourceKey(igb0)]).toEqual({ atMs: Date.parse(secondAt), rxBytes: null, txBytes: null })

    history = appendOpnsenseTrafficSnapshot(history, snapshot(thirdAt, [iface("igb0", 2000, 2000)], {
      wireguard: wireguard([]), openvpn: openvpn([session("road-1", 2000, 1000)]),
      ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
    }, true, false))
    expect(history.rates[trafficSourceKey(igb0)].rx.at(-1)).toBeNull()
    expect(history.rates[trafficSourceKey(road)].rx.at(-1)).toBeNull()

    history = appendOpnsenseTrafficSnapshot(history, snapshot("2026-10-24T12:00:15Z", [iface("igb0", 3000, 3000)]))
    expect(history.rates[trafficSourceKey(igb0)].rx.at(-1)).toBe(1600)
  })

  it("handles disappearing sources, counter resets, zero deltas, and recovery after gaps", () => {
    let history = select(igb0)
    history = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt))
    history = appendOpnsenseTrafficSnapshot(history, snapshot(secondAt, []))
    history = appendOpnsenseTrafficSnapshot(history, snapshot(thirdAt, [iface("igb0", 1000, 500)]))
    expect(history.rates[trafficSourceKey(igb0)].rx).toEqual([null, null, null])

    history = appendOpnsenseTrafficSnapshot(history, snapshot("2026-10-24T12:00:15Z", [iface("igb0", 900, 500)]))
    expect(history.rates[trafficSourceKey(igb0)].rx.at(-1)).toBeNull()
    expect(history.rates[trafficSourceKey(igb0)].tx.at(-1)).toBe(0)
    history = appendOpnsenseTrafficSnapshot(history, snapshot("2026-10-24T12:00:20Z", [iface("igb0", 1900, 500)]))
    expect(history.rates[trafficSourceKey(igb0)]).toEqual({
      rx: [null, null, null, null, 1600], tx: [null, null, null, 0, 0],
    })
  })

  it("skips duplicate and malformed timestamps and resets empty history on removal", () => {
    let history = select(igb0)
    history = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt))
    const duplicate = appendOpnsenseTrafficSnapshot(history, snapshot(firstAt, [iface("igb0", 1000, 500)]))
    const invalid = appendOpnsenseTrafficSnapshot(duplicate, snapshot("not-a-date", [iface("igb0", 2000, 1000)]))
    expect(invalid).toBe(duplicate)
    expect(invalid.times).toHaveLength(1)
    expect(invalid.rates[trafficSourceKey(igb0)].rx).toEqual([null])

    history = toggleOpnsenseTrafficSource(invalid, igb0, null)
    expect(history).toEqual(createOpnsenseTrafficHistory())
    expect(appendOpnsenseTrafficSnapshot(history, snapshot(secondAt))).toBe(history)
    expect(trafficSourceKey(wg)).not.toBe(trafficSourceKey({ ...wg, interfaceName: 'wg0","remote office' }))
  })

  it("trims every selected series in alignment with the time axis", () => {
    let history = select(igb0)
    history = toggleOpnsenseTrafficSource(history, road, { at: firstAt, rxBytes: 0, txBytes: 0 })
    for (let index = 0; index <= OPNSENSE_TRAFFIC_CAPACITY; index += 1) {
      const at = new Date(Date.parse(firstAt) + index * 5000).toISOString()
      history = appendOpnsenseTrafficSnapshot(history, snapshot(at, [iface("igb0", index * 1000, index * 500)], {
        wireguard: wireguard([]), openvpn: openvpn([session("road-1", index * 2000, index * 1000)]),
        ipsec: { state: "unavailable" }, tailscale: { state: "unavailable" },
      }))
    }
    expect(history.times).toHaveLength(OPNSENSE_TRAFFIC_CAPACITY)
    expect(history.times[0]).toBe(Date.parse(secondAt) / 1000)
    for (const source of [igb0, road]) {
      const series = history.rates[trafficSourceKey(source)]
      expect(series.rx).toHaveLength(OPNSENSE_TRAFFIC_CAPACITY)
      expect(series.tx).toHaveLength(OPNSENSE_TRAFFIC_CAPACITY)
      expect(series.rx[0]).toBe(source.kind === "interface" ? 1600 : 3200)
      expect(series.rx.at(-1)).toBe(source.kind === "interface" ? 1600 : 3200)
    }
  })
})
