import type { MikrotikLoadedSessionDto, MikrotikProfile, MikrotikSessionSummaryDto, MikrotikSnapshotEvent, MikrotikVersionFirmwareResultDto } from "../lib/types"

export const profiles: readonly MikrotikProfile[] = [{ id: 7, name: "lab-router", host: "192.0.2.1", port: 8728, useTls: false, allowInvalidCerts: false, username: "admin", hasPassword: true, createdAt: "2026-09-06T11:00:00Z" }]

export const sessions: readonly MikrotikSessionSummaryDto[] = [{ id: 22, profileId: 7, startedAt: "2026-09-06T12:00:00Z", endedAt: null, status: "running", boardName: "RB5009", routerosVersion: "7.16", architectureName: "arm64", updateStatusJson: null, firmwareStatusJson: null, snapshotCount: 2 }]

export const updateResult: MikrotikVersionFirmwareResultDto = {
  updateStatus: { installedVersion: "7.16", latestVersion: "7.17", channel: "stable", status: "new-version-available" },
  firmwareStatus: { state: "available", currentFirmware: "7.16", upgradeFirmware: "7.17", model: "RB5009" },
}

export function liveSnapshot(at: string, rxBitsPerSecond: number | null): MikrotikSnapshotEvent {
  return {
    event: "snapshot",
    sessionId: 31,
    at,
    resources: { cpuLoad: 12, memUsedBytes: 200, memTotalBytes: 400, uptime: "2h" },
    sensors: [{ name: "cpu-temperature", value: 44, unit: "C", kind: "temperature" }],
    sensorsSupported: true,
    interfaces: [{
      name: "ether1", type: "ether", running: true, disabled: false, rxByte: 1_000, txByte: 2_000,
      rxPacket: null, txPacket: null, txQueueDrop: null, linkDowns: null, rxError: null, txError: null,
      rxDrop: null, rxErrorEvents: null, txErrorEvents: null, rxFcsError: null, rxAlignError: null,
      txCollision: null, txDrop: null, rate: "1G", fullDuplex: true, rxBitsPerSecond, txBitsPerSecond: 2_000,
    }],
    vlans: [{ name: "vlan10", vlanId: 10, interface: "bridge", running: true, disabled: false }],
    bridgeVlans: [{ bridge: "bridge", vlanIds: ["10"], tagged: ["ether1"], untagged: [], currentTagged: ["ether1"], currentUntagged: [] }],
    warning: null,
  }
}

export function loadedMikrotikSession(): MikrotikLoadedSessionDto {
  return {
    session: { ...sessions[0], boardName: "CCR2004", routerosVersion: "7.15.3", architectureName: "arm64", updateStatusJson: JSON.stringify(updateResult.updateStatus), firmwareStatusJson: JSON.stringify(updateResult.firmwareStatus) },
    snapshots: [
      {
        id: 1,
        sessionId: 22,
        at: "2026-09-06T12:00:00Z",
        cpuLoad: 30,
        memUsedBytes: 100,
        memTotalBytes: 200,
        uptime: "1h",
        warning: null,
        sensorsJson: JSON.stringify([{ name: "voltage", value: 24, unit: "V", kind: "voltage" }]),
        interfacesJson: JSON.stringify([liveSnapshot("2026-09-06T12:00:00Z", null).interfaces[0]]),
        vlansJson: JSON.stringify([{ name: "vlan20", vlanId: 20, interface: "bridge", running: true, disabled: false }]),
        bridgeVlansJson: JSON.stringify(liveSnapshot("2026-09-06T12:00:00Z", null).bridgeVlans),
      },
      {
        id: 2,
        sessionId: 22,
        at: "2026-09-06T12:00:07Z",
        cpuLoad: 31,
        memUsedBytes: 110,
        memTotalBytes: 200,
        uptime: "1h7s",
        warning: null,
        sensorsJson: JSON.stringify([{ name: "voltage", value: 24.2, unit: "V", kind: "voltage" }]),
        interfacesJson: JSON.stringify([{ ...liveSnapshot("2026-09-06T12:00:00Z", null).interfaces[0], rxByte: 1_700, txByte: 2_700 }]),
        vlansJson: null,
        bridgeVlansJson: null,
      },
    ],
  }
}
