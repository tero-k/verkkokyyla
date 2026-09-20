import type { Page } from "@playwright/test"

export type MockProbeEvent = {
  seq: number
  rttMs: number | null
  lost: boolean
  at: string
}

type MockTraceHop = {
  hop: number
  address: string | null
  hostname: string | null
  rtt1Ms: number | null
  rtt2Ms: number | null
  rtt3Ms: number | null
  annotation: string | null
  at: string
}

type MockTraceSummary = {
  id: number
  targetInput: string
  resolvedIp: string
  family: string
  engine: string
  maxHops: number
  startedAt: string
  endedAt: string | null
  status: string
  reachedTarget: boolean
  hopCount: number
}

type MockTrace = {
  trace: MockTraceSummary
  hops: MockTraceHop[]
}

type ActiveTrace = {
  trace: MockTrace
  onEventChannel: { onmessage?: (message: unknown) => void }
  onStatusChannel: { onmessage?: (message: unknown) => void }
  timerIds: number[]
}

type MockInterface = {
  name: string
  description: string
  ipv4: string
  prefixLen: number
  isPrimary: boolean
}

type OpenPort = {
  port: number
  service: string
}

type MockScanHost = {
  ip: string
  mac: string | null
  vendor: string | null
  hostname: string | null
  foundBy: string
  at: string
  openPorts: OpenPort[]
}

type MockScanSummary = {
  id: number
  interfaceName: string
  cidr: string
  tcpFallback: boolean
  portsEnabled: boolean
  startedAt: string
  endedAt: string | null
  status: string
  hostCount: number
}

type MockScan = {
  scan: MockScanSummary
  hosts: MockScanHost[]
}

type MockDownloadSpeedSession = {
  id: number
  url: string
  mode: string
  startedAt: string
  endedAt: string
  status: string
  averageMbps: number
  totalTimeMs: number
  resultJson: string
}

type ActiveScan = {
  scan: MockScan
  onEventChannel: { onmessage?: (message: unknown) => void }
  onStatusChannel: { onmessage?: (message: unknown) => void }
  timerIds: number[]
}

type MockSession = {
  id: number
  targetInput: string
  resolvedIp: string
  family: string
  engine: string
  intervalMs: number
  timeoutMs: number
  payloadSize: number
  dontFragment: boolean
  startedAt: string
  endedAt: string
  probeCount: number
  lossCount: number
  lossPercent: number
  probes: MockProbeEvent[]
}

type Snapshot = {
  count: number
  lossCount: number
  lossFraction: number
  minMs: number | null
  avgMs: number | null
  maxMs: number | null
  stddevMs: number | null
  jitterMs: number | null
}

type MockMikrotikProfile = {
  id: number
  name: string
  host: string
  port: number
  useTls: boolean
  allowInvalidCerts: boolean
  username: string
  hasPassword: boolean
  createdAt: string
}

type MockMikrotikBackup = {
  id: number
  profileId: number | null
  profileName: string
  name: string
  backupPath: string
  exportPath: string | null
  createdAt: string
  sizeBytes: number
  hasRscExport: boolean
}

type MockMikrotikInterface = {
  name: string
  type: string | null
  running: boolean | null
  disabled: boolean | null
  rxByte: number | null
  txByte: number | null
  rxPacket: number | null
  txPacket: number | null
  txQueueDrop: number | null
  linkDowns: number | null
  rxError: number | null
  txError: number | null
  rxDrop: number | null
  rxErrorEvents: number | null
  txErrorEvents: number | null
  rxFcsError: number | null
  rxAlignError: number | null
  txCollision: number | null
  txDrop: number | null
  rate: string | null
  fullDuplex: boolean | null
  comment: string | null
  rxBitsPerSecond: number | null
  txBitsPerSecond: number | null
}

type MockMikrotikSnapshot = {
  event: "snapshot"
  sessionId: number
  at: string
  resources: {
    cpuLoad: number | null
    memUsedBytes: number | null
    memTotalBytes: number | null
    uptime: string | null
    boardName: string | null
    routerosVersion: string | null
    architectureName: string | null
  } | null
  sensors: { name: string; value: number; unit: string | null; kind: string }[] | null
  sensorsSupported: boolean
  interfaces: MockMikrotikInterface[]
  vlans: { name: string; vlanId: number | null; interface: string | null; running: boolean | null; disabled: boolean | null }[] | null
  bridgeVlans: { bridge: string | null; vlanIds: string[]; tagged: string[]; untagged: string[]; currentTagged: string[]; currentUntagged: string[] }[] | null
  warning: string | null
}

type MockMikrotikSession = {
  session: {
    id: number
    profileId: number
    startedAt: string
    endedAt: string | null
    status: string
    boardName: string | null
    routerosVersion: string | null
    architectureName: string | null
    updateStatusJson: string | null
    firmwareStatusJson: string | null
    snapshotCount: number
  }
  snapshots: MockMikrotikSnapshot[]
}

type ActiveMikrotik = {
  session: MockMikrotikSession
  onEventChannel: { onmessage?: (message: unknown) => void }
  onStatusChannel: { onmessage?: (message: unknown) => void }
  timerIds: number[]
}

type MockMtuOutcome =
  | { readonly outcome: "ok"; readonly rttMs: number }
  | { readonly outcome: "too-big"; readonly hintMtu: number | null }
  | { readonly outcome: "timeout" }
  | { readonly outcome: "error"; readonly message: string }

type MockMtuResult =
  | { readonly kind: "exact"; readonly mtu: number }
  | {
      readonly kind: "lower-bound"
      readonly mtu: number
      readonly reason:
        | { readonly reason: "timeout-above"; readonly triedMtu: number }
        | { readonly reason: "ceiling-reached" }
    }
  | { readonly kind: "unreachable" }
  | { readonly kind: "failed"; readonly message: string }

type MockMtuProbe = {
  seq: number
  payloadSize: number
  mtuSize: number
  outcome: MockMtuOutcome
  at: string
}

type MockMtuRunSummary = {
  id: number
  targetInput: string
  resolvedIp: string
  method: string
  result: MockMtuResult
  probesSent: number
  startedAt: string
  endedAt: string | null
}

type MockMtuRun = {
  run: MockMtuRunSummary
  probes: MockMtuProbe[]
}

type ActiveMtu = {
  run: MockMtuRun
  onEventChannel: { onmessage?: (message: unknown) => void }
  onStatusChannel: { onmessage?: (message: unknown) => void }
  timerIds: number[]
}

export async function installMockTauri(page: Page): Promise<void> {
  await page.addInitScript(() => {
    ;(() => {
      let callbackId = 0
      const callbacks = new Map<number, { cb: unknown; once: boolean }>()
      let nextSessionId = 0
      let lastStartedId: number | null = null
      const activeSessions = new Map<number, {
        targetInput: string
        resolvedIp: string
        family: string
        engine: string
        payloadSize: number
        dontFragment: boolean
        onProbeChannel: { onmessage?: (message: unknown) => void }
        onStatusChannel: { onmessage?: (message: unknown) => void }
        probes: MockProbeEvent[]
      }>()
      let endedSessions: MockSession[] = []
      let endedTraces: MockTrace[] = []
      if (
        typeof window !== "undefined" &&
        Array.isArray(window.__TAURI_MOCK_ENDED_SESSIONS__)
      ) {
        endedSessions.push(...window.__TAURI_MOCK_ENDED_SESSIONS__)
      }
      if (
        typeof window !== "undefined" &&
        Array.isArray(window.__TAURI_MOCK_ENDED_TRACES__)
      ) {
        endedTraces.push(...window.__TAURI_MOCK_ENDED_TRACES__)
      }
      let nextEngineIsFallback = false
      let nextTraceId = endedTraces.reduce(
        (maxId, trace) => Math.max(maxId, trace.trace.id),
        0,
      )
      let activeTrace: ActiveTrace | null = null
      let nextScanId = 0
      let activeScan: ActiveScan | null = null
      let endedScans: MockScan[] = []
      let endedDownloadSpeedSessions: MockDownloadSpeedSession[] = []
      let nextMikrotikProfileId = 1
      let nextMikrotikSessionId = 50
      let mikrotikDialogConfirm = true
      let mikrotikTestConnectionUnauthorized = false
      let mikrotikRouterboard = true
      let mikrotikVersionVariant = "available"
      let mikrotikBackupSshUnreachable = false
      let lastMikrotikBackup: Record<string, unknown> | null = null
      let mikrotikBackupCount = 0
      let mikrotikBackups: MockMikrotikBackup[] = []
      let nextMikrotikBackupId = 1
      let mikrotikBackupDestination: string | null = null
      /** Update-notifier mock state: what the backend would have returned for
          the latest GitHub release, and how many times it was asked. */
      let updateCheckResult: {
        version: string
        url: string
        current: string
      } | null = null
      let updateCheckInvokeCount = 0
      let lastOpenedUrl: string | null = null
      const mikrotikCredentials = new Map<number, string>()
      let mikrotikProfiles: MockMikrotikProfile[] = [
        {
          id: nextMikrotikProfileId,
          name: "Lab router",
          host: "router.lab",
          port: 443,
          useTls: true,
          allowInvalidCerts: true,
          username: "admin",
          hasPassword: true,
          createdAt: "2026-09-06T10:00:00.000Z",
        },
      ]
      mikrotikCredentials.set(nextMikrotikProfileId, "lab-secret")
      nextMikrotikProfileId += 1
      /** Live monitoring sessions keyed by profile id — the mock mirrors the
          backend's multi-session manager. */
      const activeMikrotiks = new Map<number, ActiveMikrotik>()
      let endedMikrotikSessions: MockMikrotikSession[] = []
      let activeMikrotikLogs: {
        profileId: number
        onEventChannel: { onmessage?: (message: unknown) => void }
        onStatusChannel: { onmessage?: (message: unknown) => void }
        intervalId: number
        nextId: number
      } | null = null
      /** Interactive SSH terminals keyed by terminal id — the mock mirrors
          the backend's terminal manager (echo shell). */
      let nextMikrotikTerminalId = 0
      const activeMikrotikTerminals = new Map<number, {
        profileId: number
        onDataChannel: { onmessage?: (message: unknown) => void }
      }>()

      // --- MTU discovery mock state -------------------------------------
      // Scripted 1420-byte link: baseline ok, bracket too-big at 1500,
      // binary search converging to adjacent payloads 1392/1393.
      const MTU_SCRIPT: ReadonlyArray<
        readonly [payloadSize: number, mtuSize: number, outcome: MockMtuOutcome]
      > = [
        [56, 84, { outcome: "ok", rttMs: 0.8 }],
        [1472, 1500, { outcome: "too-big", hintMtu: null }],
        [1010, 1038, { outcome: "ok", rttMs: 1.1 }],
        [1241, 1269, { outcome: "ok", rttMs: 1.2 }],
        [1356, 1384, { outcome: "ok", rttMs: 1.4 }],
        [1414, 1442, { outcome: "too-big", hintMtu: null }],
        [1385, 1413, { outcome: "ok", rttMs: 1.6 }],
        [1399, 1427, { outcome: "too-big", hintMtu: null }],
        [1392, 1420, { outcome: "ok", rttMs: 1.7 }],
        [1393, 1421, { outcome: "too-big", hintMtu: null }],
      ]

      function mtuTimestamp(index: number): string {
        return new Date(Date.UTC(2026, 8, 13, 12, 0, index)).toISOString()
      }

      function mtuProbesFromScript(startIndex: number): MockMtuProbe[] {
        return MTU_SCRIPT.map(([payloadSize, mtuSize, outcome], index) => ({
          seq: index + 1,
          payloadSize,
          mtuSize,
          outcome,
          at: mtuTimestamp(startIndex + index),
        }))
      }

      function makeMtuRun(
        id: number,
        targetInput: string,
        resolvedIp: string,
        method: string,
        result: MockMtuResult,
        probes: MockMtuProbe[],
        startedAt: string,
        endedAt: string | null,
      ): MockMtuRun {
        return {
          run: {
            id,
            targetInput,
            resolvedIp,
            method,
            result,
            probesSent: probes.length,
            startedAt,
            endedAt,
          },
          probes,
        }
      }

      let nextMtuRunId = 2
      let activeMtu: ActiveMtu | null = null
      let endedMtuRuns: MockMtuRun[] = [
        makeMtuRun(
          1,
          "core.example",
          "192.0.2.1",
          "icmp",
          { kind: "exact", mtu: 1420 },
          mtuProbesFromScript(0),
          mtuTimestamp(0),
          mtuTimestamp(20),
        ),
        makeMtuRun(
          2,
          "edge.example",
          "198.51.100.2",
          "tcp",
          {
            kind: "lower-bound",
            mtu: 1500,
            reason: { reason: "timeout-above", triedMtu: 1600 },
          },
          [
            {
              seq: 1,
              payloadSize: 1472,
              mtuSize: 1500,
              outcome: { outcome: "ok", rttMs: 12.4 },
              at: mtuTimestamp(30),
            },
            {
              seq: 2,
              payloadSize: 1572,
              mtuSize: 1600,
              outcome: { outcome: "timeout" },
              at: mtuTimestamp(31),
            },
          ],
          mtuTimestamp(30),
          mtuTimestamp(32),
        ),
      ]

      function transformCallback(
        cb: (rawMessage: unknown) => void,
        once = false,
      ): number {
        const id = ++callbackId
        callbacks.set(id, { cb, once })
        return id
      }

      function unregisterCallback(id: number): void {
        callbacks.delete(id)
      }

      function sendChannel(
        channel: { onmessage?: (message: unknown) => void } | null,
        message: unknown,
      ): void {
        if (channel !== null && typeof channel.onmessage === "function") {
          channel.onmessage(message)
        }
      }

      function traceTimestamp(): string {
        return new Date().toISOString()
      }

      function makeTraceHop(
        hop: number,
        address: string | null,
        rtt1Ms: number | null,
        rtt2Ms: number | null,
        rtt3Ms: number | null,
      ): MockTraceHop {
        return {
          hop,
          address,
          hostname: null,
          rtt1Ms,
          rtt2Ms,
          rtt3Ms,
          annotation: null,
          at: traceTimestamp(),
        }
      }

      function clearTraceTimers(trace: ActiveTrace): void {
        for (const timerId of trace.timerIds) {
          window.clearTimeout(timerId)
        }
        trace.timerIds = []
      }

      function sendTraceHop(trace: ActiveTrace, hop: MockTraceHop): void {
        trace.trace.hops.push(hop)
        sendChannel(trace.onEventChannel, {
          event: "hop",
          hop: hop.hop,
          address: hop.address,
          rtt1Ms: hop.rtt1Ms,
          rtt2Ms: hop.rtt2Ms,
          rtt3Ms: hop.rtt3Ms,
          annotation: hop.annotation,
          at: hop.at,
        })
      }

      function sendTraceHostname(
        trace: ActiveTrace,
        hop: number,
        hostname: string | null,
      ): void {
        const existingHop = trace.trace.hops.find((entry) => entry.hop === hop)
        if (existingHop !== undefined) {
          existingHop.hostname = hostname
        }
        sendChannel(trace.onEventChannel, {
          event: "hostname",
          hop,
          address: existingHop?.address ?? trace.trace.trace.resolvedIp,
          hostname,
        })
      }

      function scheduleTraceStep(
        trace: ActiveTrace,
        delayMs: number,
        step: () => void,
      ): void {
        trace.timerIds.push(window.setTimeout(step, delayMs))
      }

      function persistTrace(
        trace: ActiveTrace,
        status: "completed" | "cancelled",
        reachedTarget: boolean,
      ): void {
        trace.trace.trace.endedAt = traceTimestamp()
        trace.trace.trace.status = status
        trace.trace.trace.reachedTarget = reachedTarget
        trace.trace.trace.hopCount = trace.trace.hops.length
        endedTraces.push(trace.trace)
      }

      function scanTimestamp(): string {
        return new Date().toISOString()
      }

      function sendScanHost(scan: ActiveScan, host: MockScanHost): void {
        scan.scan.hosts.push(host)
        sendChannel(scan.onEventChannel, {
          event: "host",
          ip: host.ip,
          mac: host.mac,
          vendor: host.vendor,
          hostname: host.hostname,
          foundBy: host.foundBy,
          openPorts: host.openPorts,
          at: host.at,
        })
      }

      function clearScanTimers(scan: ActiveScan): void {
        for (const timerId of scan.timerIds) {
          window.clearTimeout(timerId)
        }
        scan.timerIds = []
      }

      function persistScan(scan: ActiveScan, status: string): void {
        scan.scan.scan.endedAt = scanTimestamp()
        scan.scan.scan.status = status
        scan.scan.scan.hostCount = scan.scan.hosts.length
        endedScans.push(scan.scan)
      }

      function scheduleScanStep(
        scan: ActiveScan,
        delayMs: number,
        step: () => void,
      ): void {
        scan.timerIds.push(window.setTimeout(step, delayMs))
      }

      function startExampleTrace(trace: ActiveTrace): void {
        scheduleTraceStep(trace, 0, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHop(trace, makeTraceHop(1, "192.0.2.1", 0.5, 0.5, 0.5))
        })
        scheduleTraceStep(trace, 10, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHostname(trace, 1, "router-1.example.net")
        })
        scheduleTraceStep(trace, 20, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHop(trace, makeTraceHop(2, "198.51.100.1", 12, null, 11))
        })
        scheduleTraceStep(trace, 30, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHostname(trace, 2, "router-2.example.net")
        })
        scheduleTraceStep(trace, 40, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHop(
            trace,
            makeTraceHop(3, trace.trace.trace.resolvedIp, 2, 2, 2),
          )
          persistTrace(trace, "completed", true)
          sendChannel(trace.onStatusChannel, {
            event: "completed",
            traceId: trace.trace.trace.id,
            hopCount: trace.trace.hops.length,
            reachedTarget: true,
          })
          activeTrace = null
        })
        scheduleTraceStep(trace, 50, () => {
          sendTraceHostname(trace, 3, "example.com")
        })
      }

      function startGenericTrace(trace: ActiveTrace): void {
        scheduleTraceStep(trace, 0, () => {
          if (activeTrace?.trace.trace.id !== trace.trace.trace.id) return
          sendTraceHop(
            trace,
            makeTraceHop(1, trace.trace.trace.resolvedIp, 0.5, 0.5, 0.5),
          )
          persistTrace(trace, "completed", true)
          sendChannel(trace.onStatusChannel, {
            event: "completed",
            traceId: trace.trace.trace.id,
            hopCount: trace.trace.hops.length,
            reachedTarget: true,
          })
          activeTrace = null
        })
        scheduleTraceStep(trace, 10, () => {
          sendTraceHostname(trace, 1, trace.trace.trace.targetInput)
        })
      }

      function sendMtuProbe(active: ActiveMtu, probe: MockMtuProbe): void {
        active.run.probes.push(probe)
        sendChannel(active.onEventChannel, {
          event: "attempt",
          seq: probe.seq,
          payloadSize: probe.payloadSize,
          mtuSize: probe.mtuSize,
        })
        sendChannel(active.onEventChannel, {
          event: "outcome",
          seq: probe.seq,
          outcome: probe.outcome,
        })
      }

      function scheduleMtuStep(
        active: ActiveMtu,
        delayMs: number,
        step: () => void,
      ): void {
        active.timerIds.push(window.setTimeout(step, delayMs))
      }

      function clearMtuTimers(active: ActiveMtu): void {
        for (const timerId of active.timerIds) {
          window.clearTimeout(timerId)
        }
        active.timerIds = []
      }

      function persistMtuRun(active: ActiveMtu, result: MockMtuResult): void {
        active.run.run.endedAt = new Date().toISOString()
        active.run.run.result = result
        active.run.run.probesSent = active.run.probes.length
        endedMtuRuns.push(active.run)
      }

      function startMtuScript(active: ActiveMtu): void {
        const runId = active.run.run.id
        MTU_SCRIPT.forEach(([payloadSize, mtuSize, outcome], index) => {
          scheduleMtuStep(active, 15 * (index + 1), () => {
            if (activeMtu?.run.run.id !== runId) return
            sendMtuProbe(active, {
              seq: index + 1,
              payloadSize,
              mtuSize,
              outcome,
              at: new Date().toISOString(),
            })
            if (index === MTU_SCRIPT.length - 1) {
              persistMtuRun(active, { kind: "exact", mtu: 1420 })
              sendChannel(active.onStatusChannel, {
                event: "completed",
                runId,
                result: { kind: "exact", mtu: 1420 },
                probesSent: active.run.probes.length,
              })
              activeMtu = null
            }
          })
        })
      }

      function startMtuHang(active: ActiveMtu): void {
        const runId = active.run.run.id
        // Two probes, then idle: long enough for the stop button to be used.
        const prefix = MTU_SCRIPT.slice(0, 2)
        prefix.forEach(([payloadSize, mtuSize, outcome], index) => {
          scheduleMtuStep(active, 20 * (index + 1), () => {
            if (activeMtu?.run.run.id !== runId) return
            sendMtuProbe(active, {
              seq: index + 1,
              payloadSize,
              mtuSize,
              outcome,
              at: new Date().toISOString(),
            })
          })
        })
        scheduleMtuStep(active, 600_000, () => {})
      }

      function resolveInfo(target: string): {
        resolvedIp: string
        engine: string
        fallback: boolean
      } {
        const engine = nextEngineIsFallback ? "surge-fallback" : "surge"
        const fallback = nextEngineIsFallback
        nextEngineIsFallback = false
        if (target === "localhost" || target === "127.0.0.1") {
          return { resolvedIp: "127.0.0.1", engine, fallback }
        }
        if (target === "::1") {
          return { resolvedIp: "::1", engine, fallback }
        }
        if (target === "compare-b.test") {
          return { resolvedIp: "198.51.100.1", engine, fallback }
        }
        return { resolvedIp: "192.0.2.1", engine, fallback }
      }

      function computeSnapshot(probes: MockProbeEvent[]): Snapshot {
        const count = probes.length
        const lossCount = probes.filter((p) => p.lost).length
        const rtts = probes
          .filter((p) => !p.lost && p.rttMs !== null)
          .map((p) => p.rttMs as number)
        const min = rtts.length > 0 ? Math.min(...rtts) : null
        const max = rtts.length > 0 ? Math.max(...rtts) : null
        const avg =
          rtts.length > 0 ? rtts.reduce((a, b) => a + b, 0) / rtts.length : null
        const variance =
          rtts.length > 0 && avg !== null
            ? rtts.reduce((a, b) => a + (b - avg) * (b - avg), 0) / rtts.length
            : null
        const stddev = variance !== null ? Math.sqrt(variance) : null
        let jitter: number | null = null
        for (let i = 1; i < rtts.length; i++) {
          const d = Math.abs(rtts[i] - rtts[i - 1])
          jitter = jitter === null ? d : jitter + (d - jitter) / 16
        }
        return {
          count,
          lossCount,
          lossFraction: count > 0 ? lossCount / count : 0,
          minMs: min,
          avgMs: avg,
          maxMs: max,
          stddevMs: stddev,
          jitterMs: jitter,
        }
      }

      function mikrotikTimestamp(index: number): string {
        return new Date(Date.UTC(2026, 8, 6, 11, 0, index)).toISOString()
      }

      function mikrotikVersionStatus() {
        if (mikrotikVersionVariant === "up-to-date") {
          return {
            updateStatus: { installedVersion: "7.17", latestVersion: "7.17", channel: "stable", state: "up-to-date", status: "System is already up to date" },
            firmwareStatus: { state: "up-to-date", currentFirmware: "7.17", upgradeFirmware: "7.17", model: mikrotikRouterboard ? "RB5009" : null },
          }
        }
        if (mikrotikVersionVariant === "unknown") {
          return {
            updateStatus: { installedVersion: "7.16", latestVersion: null, channel: "stable", state: "unknown", status: "unknown" },
            firmwareStatus: { state: "unknown", currentFirmware: null, upgradeFirmware: null, model: mikrotikRouterboard ? "RB5009" : null },
          }
        }
        if (mikrotikVersionVariant === "na") {
          return {
            updateStatus: { installedVersion: "7.16", latestVersion: null, channel: null, state: "unknown", status: "unknown" },
            firmwareStatus: { state: "not-applicable", currentFirmware: null, upgradeFirmware: null, model: null },
          }
        }
        return {
          updateStatus: { installedVersion: "7.16", latestVersion: "7.17", channel: "stable", state: "update-available", status: "new-version-available" },
          firmwareStatus: { state: "available", currentFirmware: "7.16", upgradeFirmware: "7.17", model: mikrotikRouterboard ? "RB5009" : null },
        }
      }

      function mikrotikInterface(name: string, index: number, fullCounters: boolean): MockMikrotikInterface {
        const base = index * 100_000
        return {
          name,
          type: name.startsWith("ether") ? "ether" : "vlan",
          running: true,
          disabled: false,
          rxByte: base + 1_000_000,
          txByte: base + 2_000_000,
          rxPacket: base / 100 + 10,
          txPacket: base / 100 + 20,
          txQueueDrop: fullCounters ? 1 : null,
          linkDowns: fullCounters ? 2 : null,
          rxError: fullCounters ? 3 : null,
          txError: fullCounters ? 4 : null,
          rxDrop: fullCounters ? 5 : null,
          rxErrorEvents: fullCounters ? 6 : null,
          txErrorEvents: fullCounters ? 7 : null,
          rxFcsError: fullCounters ? 8 : null,
          rxAlignError: fullCounters ? 0 : null,
          txCollision: fullCounters ? 9 : null,
          txDrop: fullCounters ? 10 : null,
          rate: "1Gbps",
          fullDuplex: true,
          comment: name === "ether1" ? "WAN uplink" : null,
          rxBitsPerSecond: 800_000 * index,
          txBitsPerSecond: 500_000 * index,
        }
      }

      function mikrotikSnapshot(sessionId: number, index: number): MockMikrotikSnapshot {
        const sensors = !mikrotikRouterboard || index === 2 ? null : [
          { name: "cpu-temperature", value: 44 + index, unit: "C", kind: "temperature" },
          { name: "fan1", value: 3200 + index, unit: "RPM", kind: "fan" },
          { name: "voltage", value: 24.1, unit: "V", kind: "voltage" },
        ]
        return {
          event: "snapshot",
          sessionId,
          at: mikrotikTimestamp(index),
          resources: {
            cpuLoad: 18 + index,
            memUsedBytes: 268_435_456 + index,
            memTotalBytes: 1_073_741_824,
            uptime: `${index}h 10m`,
            boardName: mikrotikRouterboard ? "RB5009" : null,
            routerosVersion: "7.16",
            architectureName: "arm64",
          },
          sensors,
          sensorsSupported: sensors !== null,
          interfaces: [mikrotikInterface("ether1", index, true), mikrotikInterface("sfp1", index + 1, false)],
          vlans: [{ name: "vlan20-guests", vlanId: 20, interface: "bridge", running: true, disabled: false }],
          bridgeVlans: [{ bridge: "bridge", vlanIds: ["20"], tagged: ["sfp1"], untagged: ["ether1"], currentTagged: ["sfp1"], currentUntagged: ["ether1"] }],
          warning: null,
        }
      }

      function mikrotikSnapshotRow(snapshot: MockMikrotikSnapshot, id: number) {
        return {
          id,
          sessionId: snapshot.sessionId,
          at: snapshot.at,
          cpuLoad: snapshot.resources?.cpuLoad ?? null,
          memUsedBytes: snapshot.resources?.memUsedBytes ?? null,
          memTotalBytes: snapshot.resources?.memTotalBytes ?? null,
          uptime: snapshot.resources?.uptime ?? null,
          boardName: snapshot.resources?.boardName ?? null,
          routerosVersion: snapshot.resources?.routerosVersion ?? null,
          architectureName: snapshot.resources?.architectureName ?? null,
          warning: snapshot.warning,
          sensorsJson: snapshot.sensors === null ? null : JSON.stringify(snapshot.sensors),
          interfacesJson: JSON.stringify(snapshot.interfaces),
          vlansJson: snapshot.vlans === null ? null : JSON.stringify(snapshot.vlans),
          bridgeVlansJson: snapshot.bridgeVlans === null ? null : JSON.stringify(snapshot.bridgeVlans),
        }
      }

      function scheduleMikrotikStep(active: ActiveMikrotik, delayMs: number, step: () => void): void {
        active.timerIds.push(window.setTimeout(step, delayMs))
      }

      function defaultSessionId(): number {
        if (lastStartedId !== null && activeSessions.has(lastStartedId)) {
          return lastStartedId
        }
        const ids = Array.from(activeSessions.keys())
        if (ids.length === 0) {
          throw new Error("no active session")
        }
        return ids[0]
      }

      function resolveActiveSession(sessionId: number | undefined) {
        const id = sessionId ?? defaultSessionId()
        const session = activeSessions.get(id)
        if (session === undefined) {
          throw new Error(`session ${id} not found`)
        }
        return { id, session }
      }

      async function invoke(
        cmd: string,
        args: Record<string, unknown>,
      ): Promise<unknown> {
        switch (cmd) {
          case "start_session": {
            const targetInput = String(args.target)
            const family = String(args.family)
            const payloadSize =
              typeof args.payloadSize === "number" ? args.payloadSize : 32
            const dontFragment =
              typeof args.dontFragment === "boolean" ? args.dontFragment : false
            const info = resolveInfo(targetInput)
            nextSessionId += 1
            const sessionId = nextSessionId
            const onProbeChannel = args.onProbe as { onmessage?: (message: unknown) => void }
            const onStatusChannel = args.onStatus as { onmessage?: (message: unknown) => void }
            const active = {
              targetInput,
              resolvedIp: info.resolvedIp,
              family,
              engine: info.engine,
              payloadSize,
              dontFragment,
              onProbeChannel,
              onStatusChannel,
              probes: [] as MockProbeEvent[],
            }
            activeSessions.set(sessionId, active)
            lastStartedId = sessionId
            sendChannel(onStatusChannel, {
              event: "engine-selected",
              engine: info.engine,
              fallback: info.fallback,
            })
            return {
              sessionId,
              engine: info.engine,
              fallback: info.fallback,
              resolvedIp: info.resolvedIp,
              answers: [info.resolvedIp],
              payloadSize,
              dontFragment,
            }
          }
          case "start_trace": {
            const targetInput = String(args.target)
            const family = String(args.family)
            if (targetInput === "error.test") {
              throw {
                kind: "unavailable",
                message: "traceroute binary not found",
              }
            }
            const resolvedIp =
              targetInput === "example.com"
                ? "203.0.113.9"
                : resolveInfo(targetInput).resolvedIp
            nextTraceId += 1
            const trace: MockTrace = {
              trace: {
                id: nextTraceId,
                targetInput,
                resolvedIp,
                family,
                engine: "tracert-mock",
                maxHops: 30,
                startedAt: traceTimestamp(),
                endedAt: null,
                status: "running",
                reachedTarget: false,
                hopCount: 0,
              },
              hops: [],
            }
            activeTrace = {
              trace,
              onEventChannel: args.onEvent as { onmessage?: (message: unknown) => void },
              onStatusChannel: args.onStatus as { onmessage?: (message: unknown) => void },
              timerIds: [],
            }
            if (targetInput === "example.com") {
              startExampleTrace(activeTrace)
            } else {
              startGenericTrace(activeTrace)
            }
            return {
              traceId: nextTraceId,
              engine: "tracert-mock",
              resolvedIp,
              answers: [resolvedIp],
            }
          }
          case "stop_trace": {
            if (activeTrace === null) {
              throw {
                kind: "not-running",
                message: "no traceroute is running",
              }
            }
            const trace = activeTrace
            clearTraceTimers(trace)
            persistTrace(trace, "cancelled", false)
            const endedAt = trace.trace.trace.endedAt
            sendChannel(trace.onStatusChannel, {
              event: "cancelled",
              traceId: trace.trace.trace.id,
              hopCount: trace.trace.hops.length,
            })
            activeTrace = null
            return {
              traceId: trace.trace.trace.id,
              hopCount: trace.trace.hops.length,
              endedAt,
            }
          }
          case "list_traces":
            return [...endedTraces].reverse().map((trace) => trace.trace)
          case "load_trace": {
            const trace = endedTraces.find((entry) => entry.trace.id === args.id)
            if (trace === undefined) {
              throw {
                kind: "trace-not-found",
                message: `no trace with id ${args.id}`,
              }
            }
            return { trace: trace.trace, hops: trace.hops }
          }
          case "delete_trace": {
            endedTraces = endedTraces.filter((trace) => trace.trace.id !== args.id)
            return null
          }
          case "start_mtu_probe": {
            if (activeMtu !== null) {
              throw {
                kind: "already-running",
                message: "an MTU discovery run is already active",
              }
            }
            const targetInput = String(args.target)
            const method = String(args.method)
            const resolvedIp =
              targetInput === "edge.example" ? "198.51.100.2" : "192.0.2.1"
            nextMtuRunId += 1
            const runId = nextMtuRunId
            const run: MockMtuRun = makeMtuRun(
              runId,
              targetInput,
              resolvedIp,
              method,
              { kind: "failed", message: "running" },
              [],
              new Date().toISOString(),
              null,
            )
            activeMtu = {
              run,
              onEventChannel: args.onEvent as {
                onmessage?: (message: unknown) => void
              },
              onStatusChannel: args.onStatus as {
                onmessage?: (message: unknown) => void
              },
              timerIds: [],
            }
            if (targetInput === "cancel.example") {
              startMtuHang(activeMtu)
            } else {
              startMtuScript(activeMtu)
            }
            return {
              runId,
              method,
              resolvedIp,
              answers: [resolvedIp],
            }
          }
          case "stop_mtu_probe": {
            if (activeMtu === null) {
              throw {
                kind: "no-active-run",
                message: "no MTU discovery run is active",
              }
            }
            const stopped = activeMtu
            clearMtuTimers(stopped)
            persistMtuRun(stopped, {
              kind: "failed",
              message: "cancelled",
            })
            activeMtu = null
            sendChannel(stopped.onStatusChannel, {
              event: "cancelled",
              runId: stopped.run.run.id,
              probesSent: stopped.run.probes.length,
            })
            return {
              runId: stopped.run.run.id,
              probesSent: stopped.run.probes.length,
            }
          }
          case "list_mtu_runs":
            return [...endedMtuRuns].reverse().map((entry) => entry.run)
          case "load_mtu_run": {
            const run = endedMtuRuns.find((entry) => entry.run.id === args.id)
            if (run === undefined) {
              throw {
                kind: "run-not-found",
                message: `no MTU run with id ${args.id}`,
              }
            }
            return { run: run.run, probes: run.probes }
          }
          case "delete_mtu_run": {
            endedMtuRuns = endedMtuRuns.filter((entry) => entry.run.id !== args.id)
            return null
          }
          case "stop_session": {
            const sessionId = Number(args.sessionId)
            const active = activeSessions.get(sessionId)
            if (active === undefined) {
              throw {
                kind: "not-running",
                message: "no ping session is running",
              }
            }
            activeSessions.delete(sessionId)
            if (lastStartedId === sessionId) {
              lastStartedId = null
            }
            const snap = computeSnapshot(active.probes)
            const endedAt = new Date().toISOString()
            const startedAt = new Date(
              Date.now() - snap.count * 1000,
            ).toISOString()
            const ended: MockSession = {
              id: sessionId,
              targetInput: active.targetInput,
              resolvedIp: active.resolvedIp,
              family: active.family,
              engine: active.engine,
              intervalMs: 1000,
              timeoutMs: 1000,
              payloadSize: active.payloadSize,
              dontFragment: active.dontFragment,
              startedAt,
              endedAt,
              probeCount: snap.count,
              lossCount: snap.lossCount,
              lossPercent: snap.lossFraction * 100,
              probes: active.probes,
            }
            endedSessions.push(ended)
            sendChannel(active.onStatusChannel, {
              event: "session-stopped",
              sessionId,
              probeCount: snap.count,
              lossCount: snap.lossCount,
            })
            return {
              sessionId,
              probeCount: snap.count,
              lossCount: snap.lossCount,
              endedAt,
            }
          }
          case "get_snapshot": {
            const sessionId = Number(args.sessionId)
            const active = activeSessions.get(sessionId)
            if (active === undefined) {
              return {
                count: 0,
                lossCount: 0,
                lossFraction: 0,
                minMs: null,
                avgMs: null,
                maxMs: null,
                stddevMs: null,
                jitterMs: null,
              }
            }
            return computeSnapshot(active.probes)
          }
          case "list_active_sessions":
            return Array.from(activeSessions.keys())
          case "list_sessions":
            return [...endedSessions].reverse()
          case "load_session": {
            const session = endedSessions.find((s) => s.id === args.id)
            if (session === undefined) {
              throw {
                kind: "session-not-found",
                message: `no session with id ${args.id}`,
              }
            }
            const probes = session.probes.map((p) => ({
              seq: p.seq,
              rttMs: p.rttMs,
              loss: p.lost,
              at: p.at,
            }))
            return { session, probes }
          }
          case "delete_session": {
            endedSessions = endedSessions.filter((s) => s.id !== args.id)
            return null
          }
          case "run_download_speed_test": {
            const url = String(args.url)
            const settings = args.settings as Record<string, unknown> | undefined
            const onProgress = args.onProgress as {
              onmessage?: (message: unknown) => void
            }
            window.__TAURI_MOCK_LAST_HTTP_SETTINGS__ = settings ?? null
            if (url === "https://error.test/") {
              throw {
                kind: "request",
                message: "request failed: mock failure",
              }
            }
            const contentLength = 1_048_576
            const chunk = contentLength / 4
            for (let i = 1; i <= 4; i++) {
              sendChannel(onProgress, {
                event: "progress",
                bytesReceived: chunk * i,
                contentLength,
                elapsedMs: 250 * i,
                currentMbps: 8.39,
              })
            }
            return {
              url,
              finalUrl: url,
              statusCode: 200,
              contentLength,
              bytesReceived: contentLength,
              totalTimeMs: 1000,
              timeToFirstByteMs: 120,
              dnsResolutionMs: 12,
              tlsHandshakeMs: null,
              averageMbps: 8.39,
            }
          }
          case "run_page_speed_test": {
            const url = String(args.url)
            const settings = args.settings as Record<string, unknown> | undefined
            const onProgress = args.onProgress as {
              onmessage?: (message: unknown) => void
            }
            window.__TAURI_MOCK_LAST_HTTP_SETTINGS__ = settings ?? null
            if (url === "https://error.test/") {
              throw {
                kind: "request",
                message: "request failed: mock page failure",
              }
            }
            const resources = [
              {
                url,
                resourceType: "document",
                statusCode: 200,
                contentLength: 4096,
                bytesReceived: 4096,
                startOffsetMs: 0,
                durationMs: 120,
                timeToFirstByteMs: 40,
                averageMbps: 0.27,
                error: null,
              },
              {
                url: new URL("/style.css", url).href,
                resourceType: "stylesheet",
                statusCode: 200,
                contentLength: 2048,
                bytesReceived: 2048,
                startOffsetMs: 130,
                durationMs: 80,
                timeToFirstByteMs: 20,
                averageMbps: 0.2,
                error: null,
              },
              {
                url: new URL("/app.js", url).href,
                resourceType: "script",
                statusCode: 200,
                contentLength: 8192,
                bytesReceived: 8192,
                startOffsetMs: 220,
                durationMs: 150,
                timeToFirstByteMs: 30,
                averageMbps: 0.44,
                error: null,
              },
              {
                url: new URL("/image.png", url).href,
                resourceType: "image",
                statusCode: 200,
                contentLength: 10240,
                bytesReceived: 10240,
                startOffsetMs: 380,
                durationMs: 210,
                timeToFirstByteMs: 50,
                averageMbps: 0.39,
                error: null,
              },
              {
                url: new URL("/font.woff2", url).href,
                resourceType: "font",
                statusCode: 200,
                contentLength: 5120,
                bytesReceived: 5120,
                startOffsetMs: 410,
                durationMs: 95,
                timeToFirstByteMs: 25,
                averageMbps: 0.43,
                error: null,
              },
              {
                // Self-referencing link, like is.fi's canonical <link href> —
                // intentionally shares the document URL to guard against
                // duplicate-key rendering bugs.
                url,
                resourceType: "other",
                statusCode: 200,
                contentLength: 4096,
                bytesReceived: 4096,
                startOffsetMs: 403,
                durationMs: 85,
                timeToFirstByteMs: 30,
                averageMbps: 0.39,
                error: null,
              },
              {
                url: new URL("/xhr.json", url).href,
                resourceType: "xhr",
                statusCode: 200,
                contentLength: 256,
                bytesReceived: 256,
                startOffsetMs: 520,
                durationMs: 45,
                timeToFirstByteMs: 15,
                averageMbps: 0.05,
                error: null,
              },
            ]
            for (let i = 0; i < resources.length; i++) {
              sendChannel(onProgress, {
                event: "progress",
                resource: resources[i],
                completed: i + 1,
                total: resources.length,
              })
            }
            const totalBytesReceived = resources.reduce(
              (sum, r) => sum + r.bytesReceived,
              0,
            )
            return {
              url,
              totalResources: resources.length,
              successfulResources: resources.length,
              failedResources: 0,
              totalBytesReceived,
              totalDurationMs: 565,
              timeToFirstByteMs: 40,
              averageMbps: 0.3,
              resources,
            }
          }
          case "list_interfaces": {
            return [
              {
                name: "eth0",
                description: "Primary Ethernet",
                ipv4: "192.168.1.10",
                prefixLen: 24,
                isPrimary: true,
              },
              {
                name: "wlan0",
                description: "Wi-Fi",
                ipv4: "192.168.2.5",
                prefixLen: 24,
                isPrimary: false,
              },
            ]
          }
          case "start_scan": {
            const interfaceName = String(args.interfaceName)
            const cidr = String(args.cidr)
            const tcpFallback =
              typeof args.tcpFallback === "boolean" ? args.tcpFallback : true
            const portsEnabled =
              typeof args.portsEnabled === "boolean" ? args.portsEnabled : false
            if (activeScan !== null) {
              throw {
                kind: "already-running",
                message: "a scan is already running",
              }
            }
            nextScanId += 1
            const scanId = nextScanId
            const startedAt = scanTimestamp()
            const scan: MockScan = {
              scan: {
                id: scanId,
                interfaceName,
                cidr,
                tcpFallback,
                portsEnabled,
                startedAt,
                endedAt: null,
                status: "running",
                hostCount: 0,
              },
              hosts: [],
            }
            activeScan = {
              scan,
              onEventChannel: args.onEvent as {
                onmessage?: (message: unknown) => void
              },
              onStatusChannel: args.onStatus as {
                onmessage?: (message: unknown) => void
              },
              timerIds: [],
            }
            const currentScan = activeScan
            const host1: MockScanHost = {
              ip: "192.168.1.1",
              mac: "AA:BB:CC:DD:EE:01",
              vendor: "Router Corp",
              hostname: "router.local",
              foundBy: "ping",
              at: startedAt,
              openPorts: [],
            }
            const host2: MockScanHost = {
              ip: "192.168.1.42",
              mac: "AA:BB:CC:DD:EE:02",
              vendor: "Example Devices",
              hostname: "laptop.local",
              foundBy: "ping",
              at: startedAt,
              openPorts: portsEnabled
                ? [
                    { port: 22, service: "ssh" },
                    { port: 80, service: "http" },
                  ]
                : [],
            }
            scheduleScanStep(currentScan, 0, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendChannel(currentScan.onStatusChannel, {
                event: "engine",
                engine: "ping+arp",
                tcpFallback,
              })
            })
            scheduleScanStep(currentScan, 5, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendChannel(currentScan.onStatusChannel, {
                event: "progress",
                done: 0,
                total: 2,
              })
            })
            scheduleScanStep(currentScan, 10, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendScanHost(currentScan, host1)
            })
            scheduleScanStep(currentScan, 15, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendChannel(currentScan.onStatusChannel, {
                event: "progress",
                done: 1,
                total: 2,
              })
            })
            scheduleScanStep(currentScan, 20, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendScanHost(currentScan, host2)
            })
            scheduleScanStep(currentScan, 25, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              sendChannel(currentScan.onStatusChannel, {
                event: "progress",
                done: 2,
                total: 2,
              })
            })
            scheduleScanStep(currentScan, 30, () => {
              if (activeScan?.scan.scan.id !== scanId) return
              persistScan(currentScan, "completed")
              sendChannel(currentScan.onStatusChannel, {
                event: "completed",
                scanId,
                hostCount: currentScan.scan.hosts.length,
              })
              activeScan = null
            })
            return {
              scanId,
              interfaceName,
              cidr,
              tcpFallback,
              portsEnabled,
            }
          }
          case "stop_scan": {
            if (activeScan === null) {
              throw {
                kind: "no-active-scan",
                message: "no scan is running",
              }
            }
            const scan = activeScan
            clearScanTimers(scan)
            persistScan(scan, "stopped")
            const endedAt = scan.scan.scan.endedAt
            const hostCount = scan.scan.hosts.length
            sendChannel(scan.onStatusChannel, {
              event: "stopped",
              scanId: scan.scan.scan.id,
              hostCount,
            })
            activeScan = null
            return {
              scanId: scan.scan.scan.id,
              hostCount,
              endedAt,
            }
          }
          case "list_scans":
            return [...endedScans].reverse().map((scan) => scan.scan)
          case "load_scan": {
            const scan = endedScans.find((entry) => entry.scan.id === args.id)
            if (scan === undefined) {
              throw {
                kind: "scan-not-found",
                message: `no scan with id ${args.id}`,
              }
            }
            return { scan: scan.scan, hosts: scan.hosts }
          }
          case "delete_scan": {
            endedScans = endedScans.filter((scan) => scan.scan.id !== args.id)
            return null
          }
          case "save_download_speed_session": {
            const payload = args.request ?? args
            const url = String(payload.url)
            const mode = String(payload.mode)
            const resultJson = String(payload.resultJson)
            const averageMbps = Number(payload.averageMbps)
            const totalTimeMs = Number(payload.totalTimeMs)
            const startedAt = new Date().toISOString()
            const id = endedDownloadSpeedSessions.length + 1
            endedDownloadSpeedSessions.push({
              id,
              url,
              mode,
              startedAt,
              endedAt: startedAt,
              status: "completed",
              averageMbps,
              totalTimeMs,
              resultJson,
            })
            return {
              id,
              url,
              mode,
              startedAt,
              endedAt: startedAt,
              status: "completed",
              averageMbps,
              totalTimeMs,
            }
          }
          case "list_download_speed_sessions": {
            return [...endedDownloadSpeedSessions].reverse().map((s) => ({
              id: s.id,
              url: s.url,
              mode: s.mode,
              startedAt: s.startedAt,
              endedAt: s.endedAt,
              status: s.status,
              averageMbps: s.averageMbps,
              totalTimeMs: s.totalTimeMs,
            }))
          }
          case "load_download_speed_session": {
            const session = endedDownloadSpeedSessions.find((s) => s.id === args.id)
            if (session === undefined) {
              throw {
                kind: "session-not-found",
                message: `no download speed session with id ${args.id}`,
              }
            }
            return {
              session: {
                id: session.id,
                url: session.url,
                mode: session.mode,
                startedAt: session.startedAt,
                endedAt: session.endedAt,
                status: session.status,
                averageMbps: session.averageMbps,
                totalTimeMs: session.totalTimeMs,
              },
              resultJson: session.resultJson,
            }
          }
          case "delete_download_speed_session": {
            endedDownloadSpeedSessions = endedDownloadSpeedSessions.filter(
              (s) => s.id !== args.id,
            )
            return null
          }
          case "plugin:dialog|open":
            return mikrotikDialogConfirm ? "C:/verkkokyyla-e2e/backups" : null
          case "check_for_update": {
            updateCheckInvokeCount += 1
            return updateCheckResult
          }
          case "app_version":
            return "0.1.3"
          case "plugin:opener|open_url": {
            lastOpenedUrl = String(args.url)
            return null
          }
          case "mikrotik_list_profiles":
            return mikrotikProfiles
          case "mikrotik_create_profile": {
            const request = args.request as Record<string, unknown>
            const profile: MockMikrotikProfile = {
              id: nextMikrotikProfileId,
              name: String(request.name),
              host: String(request.host),
              port: Number(request.port),
              useTls: Boolean(request.useTls),
              allowInvalidCerts: Boolean(request.allowInvalidCerts),
              username: String(request.username),
              hasPassword: false,
              createdAt: mikrotikTimestamp(nextMikrotikProfileId),
            }
            nextMikrotikProfileId += 1
            mikrotikProfiles.push(profile)
            return profile
          }
          case "mikrotik_update_profile": {
            const request = args.request as Record<string, unknown>
            const id = Number(request.id)
            const existing = mikrotikProfiles.find((profile) => profile.id === id)
            if (existing === undefined) throw { kind: "NotFound", message: "MikroTik profile not found" }
            const updated = { ...existing, name: String(request.name), host: String(request.host), port: Number(request.port), useTls: Boolean(request.useTls), allowInvalidCerts: Boolean(request.allowInvalidCerts), username: String(request.username) }
            mikrotikProfiles = mikrotikProfiles.map((profile) => profile.id === id ? updated : profile)
            return updated
          }
          case "mikrotik_delete_profile": {
            const id = Number(args.id)
            mikrotikProfiles = mikrotikProfiles.filter((profile) => profile.id !== id)
            const secretDeleted = mikrotikCredentials.delete(id)
            return { deleted: true, secretDeleted, warning: null }
          }
          case "mikrotik_set_profile_password": {
            const id = Number(args.id)
            mikrotikCredentials.set(id, String(args.password))
            mikrotikProfiles = mikrotikProfiles.map((profile) => profile.id === id ? { ...profile, hasPassword: true } : profile)
            return null
          }
          case "mikrotik_test_connection": {
            if (mikrotikTestConnectionUnauthorized) throw { kind: "HttpStatus", status: 401, message: "MikroTik API returned 401 Unauthorized" }
            return { boardName: mikrotikRouterboard ? "RB5009" : null, routerosVersion: "7.16", architectureName: "arm64" }
          }
          case "mikrotik_start": {
            const profileId = Number(args.profileId)
            if (activeMikrotiks.has(profileId)) throw { kind: "AlreadyRunning", message: "this profile is already being monitored" }
            nextMikrotikSessionId += 1
            const status = mikrotikVersionStatus()
            const session: MockMikrotikSession = { session: { id: nextMikrotikSessionId, profileId, startedAt: mikrotikTimestamp(0), endedAt: null, status: "running", boardName: mikrotikRouterboard ? "RB5009" : null, routerosVersion: "7.16", architectureName: "arm64", updateStatusJson: null, firmwareStatusJson: null, snapshotCount: 0 }, snapshots: [] }
            const current: ActiveMikrotik = { session, onEventChannel: args.onEvent as { onmessage?: (message: unknown) => void }, onStatusChannel: args.onStatus as { onmessage?: (message: unknown) => void }, timerIds: [] }
            activeMikrotiks.set(profileId, current)
            sendChannel(current.onStatusChannel, { event: "started", sessionId: session.session.id, profileId })
            scheduleMikrotikStep(current, 10, () => {
              if (activeMikrotiks.get(profileId)?.session.session.id !== session.session.id) return
              sendChannel(current.onStatusChannel, { event: "version-firmware", sessionId: session.session.id, updateStatus: status.updateStatus, firmwareStatus: status.firmwareStatus })
              session.session.updateStatusJson = JSON.stringify(status.updateStatus)
              session.session.firmwareStatusJson = JSON.stringify(status.firmwareStatus)
            })
            for (let index = 1; index <= 3; index += 1) {
              scheduleMikrotikStep(current, 15 * index, () => {
                if (activeMikrotiks.get(profileId)?.session.session.id !== session.session.id) return
                const snapshot = mikrotikSnapshot(session.session.id, index)
                session.snapshots.push(snapshot)
                session.session.snapshotCount = session.snapshots.length
                sendChannel(current.onEventChannel, snapshot)
              })
            }
            return { sessionId: session.session.id, profileId }
          }
          case "mikrotik_stop": {
            const sessionId = Number(args.sessionId)
            const entry = Array.from(activeMikrotiks.entries()).find(([, active]) => active.session.session.id === sessionId)
            if (entry === undefined) throw { kind: "NoActiveSession", message: "no MikroTik session is running with that id" }
            const [profileId, stopped] = entry
            for (const timerId of stopped.timerIds) window.clearTimeout(timerId)
            stopped.session.session.endedAt = mikrotikTimestamp(9)
            stopped.session.session.status = "stopped"
            stopped.session.session.snapshotCount = stopped.session.snapshots.length
            endedMikrotikSessions.push(stopped.session)
            activeMikrotiks.delete(profileId)
            sendChannel(stopped.onStatusChannel, { event: "stopped", sessionId: stopped.session.session.id, snapshotCount: stopped.session.snapshots.length })
            return { sessionId: stopped.session.session.id, snapshotCount: stopped.session.snapshots.length, endedAt: stopped.session.session.endedAt, status: "stopped" }
          }
          case "mikrotik_list_active":
            return Array.from(activeMikrotiks.values()).map((active) => ({ sessionId: active.session.session.id, profileId: active.session.session.profileId }))
          case "mikrotik_list_sessions":
            return [...endedMikrotikSessions].reverse().map((entry) => entry.session)
          case "mikrotik_log_start": {
            const profileId = Number(args.profileId)
            const onEventChannel = args.onEvent as { onmessage?: (message: unknown) => void }
            const onStatusChannel = args.onStatus as { onmessage?: (message: unknown) => void }
            sendChannel(onStatusChannel, { event: "started", profileId })
            activeMikrotikLogs = { profileId, onEventChannel, onStatusChannel, intervalId: 0, nextId: 100 }
            const pushEntry = () => {
              const current = activeMikrotikLogs
              if (current === null || current.profileId !== profileId) return
              current.nextId += 1
              const topics =
                current.nextId % 5 === 0
                  ? ["system", "error"]
                  : current.nextId % 7 === 0
                    ? ["dhcp", "warning"]
                    : ["system", "info"]
              const severity = topics.includes("error") ? "error" : topics.includes("warning") ? "warning" : "info"
              sendChannel(current.onEventChannel, {
                event: "entries",
                entries: [
                  {
                    id: `*${current.nextId}`,
                    time: "12:52:24",
                    topics,
                    message: `mock log entry ${current.nextId}`,
                    severity,
                  },
                ],
              })
            }
            // Backlog, then one entry per tick at the requested cadence
            // (floored so e2e tests stay fast even at 30 s settings).
            pushEntry()
            activeMikrotikLogs.intervalId = window.setInterval(pushEntry, Math.max(Number(args.pollSeconds) * 1000, 100))
            return { profileId }
          }
          case "mikrotik_log_stop": {
            const profileId = Number(args.profileId)
            if (activeMikrotikLogs === null || activeMikrotikLogs.profileId !== profileId) throw { kind: "NoActiveSession", message: "no MikroTik log stream is running for that profile" }
            window.clearInterval(activeMikrotikLogs.intervalId)
            sendChannel(activeMikrotikLogs.onStatusChannel, { event: "stopped" })
            activeMikrotikLogs = null
            return null
          }
          case "mikrotik_terminal_open": {
            const profileId = Number(args.profileId)
            if (!mikrotikProfiles.some((item) => item.id === profileId)) throw { kind: "ProfileNotFound", message: `no mikrotik profile with id ${profileId}` }
            nextMikrotikTerminalId += 1
            const terminalId = nextMikrotikTerminalId
            const onDataChannel = args.onData as { onmessage?: (message: unknown) => void }
            activeMikrotikTerminals.set(terminalId, { profileId, onDataChannel })
            // A fake shell banner; writes are echoed back below. Chunks are
            // base64 in both directions, like the real backend.
            window.setTimeout(() => {
              if (!activeMikrotikTerminals.has(terminalId)) return
              sendChannel(onDataChannel, btoa("MikroTik RouterOS mock terminal\r\n[admin@MikroTik] > "))
            }, 20)
            return { terminalId, profileId }
          }
          case "mikrotik_terminal_write": {
            const terminalId = Number(args.terminalId)
            const terminal = activeMikrotikTerminals.get(terminalId)
            if (terminal === undefined) throw { kind: "NoActiveSession", message: "no mikrotik terminal is running with that id" }
            const data = String(args.data)
            window.setTimeout(() => {
              if (activeMikrotikTerminals.has(terminalId)) sendChannel(terminal.onDataChannel, data)
            }, 5)
            return null
          }
          case "mikrotik_terminal_resize":
            return null
          case "mikrotik_terminal_close": {
            if (!activeMikrotikTerminals.delete(Number(args.terminalId))) throw { kind: "NoActiveSession", message: "no mikrotik terminal is running with that id" }
            return null
          }
          case "mikrotik_load_session": {
            const entry = endedMikrotikSessions.find((session) => session.session.id === args.id)
            if (entry === undefined) throw { kind: "NotFound", message: "MikroTik session not found" }
            return { session: entry.session, snapshots: entry.snapshots.map(mikrotikSnapshotRow) }
          }
          case "mikrotik_delete_session": {
            endedMikrotikSessions = endedMikrotikSessions.filter((session) => session.session.id !== args.id)
            return null
          }
          case "mikrotik_check_updates":
            return mikrotikVersionStatus()
          case "mikrotik_changelog":
            return { version: String(args.version), changelog: "RouterOS mock changelog" }
          case "mikrotik_backup": {
            lastMikrotikBackup = args
            mikrotikBackupCount += 1
            if (mikrotikBackupSshUnreachable) throw { kind: "SshUnreachable", message: "SSH connection refused" }
            const name = String(args.backupName)
            const destination = String(args.destinationDir)
            if (!Boolean(args.overwrite) && name === "existing") throw { kind: "OutputExists", message: "backup output already exists" }
            const profile = mikrotikProfiles.find((item) => item.id === Number(args.profileId))
            const includeRsc = Boolean(args.includeRsc)
            mikrotikBackups = [
              {
                id: nextMikrotikBackupId,
                profileId: profile?.id ?? null,
                profileName: profile?.name ?? "Unknown profile",
                name,
                backupPath: `${destination}/${name}.backup`,
                exportPath: includeRsc ? `${destination}/${name}.rsc` : null,
                createdAt: new Date().toISOString(),
                sizeBytes: 2048,
                hasRscExport: includeRsc,
              },
              ...mikrotikBackups,
            ]
            nextMikrotikBackupId += 1
            return { backupPath: `${destination}/${name}.backup`, exportPath: includeRsc ? `${destination}/${name}.rsc` : null, cleanupWarnings: [] }
          }
          case "mikrotik_list_backups":
            return mikrotikBackups
          case "mikrotik_delete_backup": {
            const id = Number(args.id)
            if (!mikrotikBackups.some((backup) => backup.id === id)) {
              throw { kind: "BackupRecordNotFound", message: `no MikroTik backup record with id ${id}` }
            }
            mikrotikBackups = mikrotikBackups.filter((backup) => backup.id !== id)
            return { deleted: true, warnings: [] }
          }
          case "mikrotik_get_backup_destination":
            return mikrotikBackupDestination
          case "mikrotik_set_backup_destination":
            mikrotikBackupDestination = String(args.path)
            return null
          case "mikrotik_diff_backups": {
            const olderId = Number(args.olderId)
            const newerId = Number(args.newerId)
            const older = mikrotikBackups.find((backup) => backup.id === olderId)
            const newer = mikrotikBackups.find((backup) => backup.id === newerId)
            if (!older || !newer) {
              throw { kind: "BackupRecordNotFound", message: "no MikroTik backup record" }
            }
            if (!older.hasRscExport || !newer.hasRscExport) {
              throw { kind: "MissingExport", message: "backup has no .rsc export to diff" }
            }
            return {
              olderId,
              olderName: older.name,
              olderCreatedAt: older.createdAt,
              newerId,
              newerName: newer.name,
              newerCreatedAt: newer.createdAt,
              lines: [
                { kind: "same", text: "/interface bridge" },
                { kind: "same", text: "/ip firewall filter" },
                { kind: "remove", text: "add chain=forward action=drop" },
                { kind: "add", text: "add chain=forward action=accept" },
                { kind: "add", text: "/interface wireguard peers add comment=laptop" },
              ],
              added: 2,
              removed: 1,
            }
          }
          default:
            throw new Error(`unknown command ${cmd}`)
        }
      }

      window.__TAURI_INTERNALS__ = {
        transformCallback,
        unregisterCallback,
        invoke,
        isTauri: () => true,
      }

      window.__TAURI_MOCK_SEND_PROBE__ = (event, sessionId) => {
        const { session } = resolveActiveSession(sessionId)
        session.probes.push(event)
        sendChannel(session.onProbeChannel, event)
      }

      window.__TAURI_MOCK_SEND_PROBES__ = (events, sessionId) => {
        const { session } = resolveActiveSession(sessionId)
        for (const event of events) {
          session.probes.push(event)
          sendChannel(session.onProbeChannel, event)
        }
      }

      window.__TAURI_MOCK_SEND_STATUS_ERROR__ = (message, sessionId) => {
        const { session } = resolveActiveSession(sessionId)
        sendChannel(session.onStatusChannel, { event: "error", message })
      }

      window.__TAURI_MOCK_SET_FALLBACK__ = (enabled) => {
        nextEngineIsFallback = enabled
      }

      window.__TAURI_MOCK_SET_DIALOG_CONFIRM__ = (enabled) => {
        mikrotikDialogConfirm = enabled
      }

      window.__TAURI_MOCK_SET_MIKROTIK_TEST_401__ = (enabled) => {
        mikrotikTestConnectionUnauthorized = enabled
      }

      window.__TAURI_MOCK_SET_MIKROTIK_ROUTERBOARD__ = (enabled) => {
        mikrotikRouterboard = enabled
      }

      window.__TAURI_MOCK_SET_MIKROTIK_VERSION_VARIANT__ = (variant) => {
        mikrotikVersionVariant = variant
      }

      window.__TAURI_MOCK_SET_MIKROTIK_BACKUP_SSH_ERROR__ = (enabled) => {
        mikrotikBackupSshUnreachable = enabled
      }

      window.__TAURI_MOCK_LAST_MIKROTIK_BACKUP__ = () => lastMikrotikBackup

      window.__TAURI_MOCK_MIKROTIK_BACKUP_COUNT__ = () => mikrotikBackupCount

      window.__TAURI_MOCK_SET_UPDATE_CHECK_RESULT__ = (result) => {
        updateCheckResult = result
      }

      window.__TAURI_MOCK_UPDATE_CHECK_COUNT__ = () => updateCheckInvokeCount

      window.__TAURI_MOCK_LAST_OPENED_URL__ = () => lastOpenedUrl
    })()
  })
}

declare global {
  interface Window {
    __TAURI_MOCK_SEND_PROBE__: (event: MockProbeEvent, sessionId?: number) => void
    __TAURI_MOCK_SEND_PROBES__: (events: MockProbeEvent[], sessionId?: number) => void
    __TAURI_MOCK_SEND_STATUS_ERROR__: (message: string, sessionId?: number) => void
    __TAURI_MOCK_SET_FALLBACK__: (enabled: boolean) => void
    __TAURI_MOCK_SET_DIALOG_CONFIRM__: (enabled: boolean) => void
    __TAURI_MOCK_SET_MIKROTIK_TEST_401__: (enabled: boolean) => void
    __TAURI_MOCK_SET_MIKROTIK_ROUTERBOARD__: (enabled: boolean) => void
    __TAURI_MOCK_SET_MIKROTIK_VERSION_VARIANT__: (variant: string) => void
    __TAURI_MOCK_SET_MIKROTIK_BACKUP_SSH_ERROR__: (enabled: boolean) => void
    __TAURI_MOCK_LAST_MIKROTIK_BACKUP__: () => unknown
    __TAURI_MOCK_MIKROTIK_BACKUP_COUNT__: () => number
    __TAURI_MOCK_SET_UPDATE_CHECK_RESULT__: (result: {
      version: string
      url: string
      current: string
    } | null) => void
    __TAURI_MOCK_UPDATE_CHECK_COUNT__: () => number
    __TAURI_MOCK_LAST_OPENED_URL__: () => string | null
    __TAURI_MOCK_ENDED_SESSIONS__?: MockSession[]
    __TAURI_MOCK_ENDED_TRACES__?: MockTrace[]
    __TAURI_MOCK_LAST_HTTP_SETTINGS__?: Record<string, unknown> | null
  }
}

export function makeProbe(
  seq: number,
  rtt: number | null,
  lost: boolean,
): MockProbeEvent {
  return { seq, rttMs: rtt, lost, at: new Date(seq * 1000).toISOString() }
}
