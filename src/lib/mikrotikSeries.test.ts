import { describe, expect, it } from "vitest"
import {
  buildMikrotikHistory,
  buildMikrotikSnapshot,
  MIKROTIK_SERIES_CAPACITY,
} from "./mikrotikSeries"
import type { MikrotikSnapshotDto } from "./types"

const firstAt = "2026-09-06T12:00:00Z"
const secondAt = "2026-09-06T12:00:07Z"

function persistedSnapshot(
  at: string,
  interfacesJson: string,
  vlansJson: string | null = null,
): MikrotikSnapshotDto {
  return {
    id: 1,
    sessionId: 10,
    at,
    cpuLoad: 25,
    memUsedBytes: 100,
    memTotalBytes: 200,
    uptime: "1h",
    warning: null,
    sensorsJson: JSON.stringify([{ name: "temperature", value: 41, unit: "C", kind: "temperature" }]),
    interfacesJson,
    vlansJson,
    bridgeVlansJson: JSON.stringify([
      {
        bridge: "bridge",
        vlanIds: ["10"],
        tagged: ["ether1"],
        untagged: [],
        currentTagged: ["ether1"],
        currentUntagged: [],
      },
    ]),
  }
}

function iface(name: string, rxByte: number, txByte: number) {
  return {
    name,
    type: "ether",
    running: true,
    disabled: false,
    rxByte,
    txByte,
    rxPacket: null,
    txPacket: null,
    txQueueDrop: null,
    linkDowns: null,
    rxError: null,
    txError: null,
    rxDrop: null,
    rxErrorEvents: null,
    txErrorEvents: null,
    rxFcsError: null,
    rxAlignError: null,
    txCollision: null,
    txDrop: null,
    rate: null,
    fullDuplex: null,
    rxBitsPerSecond: null,
    txBitsPerSecond: null,
  }
}

describe("buildMikrotikHistory", () => {
  it("recomputes historical rates from actual elapsed seconds", () => {
    const history = buildMikrotikHistory([
      persistedSnapshot(firstAt, JSON.stringify([iface("ether1", 100, 500)])),
      persistedSnapshot(secondAt, JSON.stringify([iface("ether1", 800, 1_200)])),
    ])

    expect(history.rateSeries.ether1).toEqual([
      { at: firstAt, rxBitsPerSecond: null, txBitsPerSecond: null },
      { at: secondAt, rxBitsPerSecond: 800, txBitsPerSecond: 800 },
    ])
  })

  it("uses null rates when a persisted counter resets", () => {
    const history = buildMikrotikHistory([
      persistedSnapshot(firstAt, JSON.stringify([iface("ether1", 1_000, 1_000)])),
      persistedSnapshot(secondAt, JSON.stringify([iface("ether1", 900, 1_200)])),
    ])

    expect(history.rateSeries.ether1?.[1]).toEqual({
      at: secondAt,
      rxBitsPerSecond: null,
      txBitsPerSecond: 1_600 / 7,
    })
  })

  it("retains the last-known VLAN tables when later persisted snapshots carry null VLANs", () => {
    const vlanPayload = JSON.stringify([
      { name: "vlan10", vlanId: 10, interface: "bridge", running: true, disabled: false },
    ])
    const history = buildMikrotikHistory([
      persistedSnapshot(firstAt, JSON.stringify([iface("ether1", 100, 100)]), vlanPayload),
      persistedSnapshot(secondAt, JSON.stringify([iface("ether1", 200, 200)]), null),
    ])

    expect(history.vlans).toEqual([
      { name: "vlan10", vlanId: 10, interface: "bridge", running: true, disabled: false },
    ])
    expect(history.bridgeVlans).toEqual([
      {
        bridge: "bridge",
        vlanIds: ["10"],
        tagged: ["ether1"],
        untagged: [],
        currentTagged: ["ether1"],
        currentUntagged: [],
      },
    ])
  })

  it("caps each interface series to the ring capacity", () => {
    const snapshots = Array.from({ length: MIKROTIK_SERIES_CAPACITY + 1 }, (_, index) =>
      persistedSnapshot(
        `2026-09-06T12:${String(Math.floor(index / 60)).padStart(2, "0")}:${String(index % 60).padStart(2, "0")}Z`,
        JSON.stringify([iface("ether1", index * 10, index * 20)]),
      ),
    )

    const history = buildMikrotikHistory(snapshots)

    expect(history.rateSeries.ether1).toHaveLength(MIKROTIK_SERIES_CAPACITY)
    expect(history.rateSeries.ether1?.[0]?.at).toBe("2026-09-06T12:00:01Z")
  })
})

describe("buildMikrotikSnapshot", () => {
  it("maps a persisted row to a live-shaped snapshot", () => {
    const snapshot = buildMikrotikSnapshot(
      persistedSnapshot(firstAt, JSON.stringify([iface("ether1", 100, 200)])),
    )

    expect(snapshot).toMatchObject({
      event: "snapshot",
      sessionId: 10,
      at: firstAt,
      resources: { cpuLoad: 25, memUsedBytes: 100, memTotalBytes: 200, uptime: "1h" },
      sensorsSupported: true,
      interfaces: [{ name: "ether1", rxByte: 100, txByte: 200 }],
    })
  })
})
