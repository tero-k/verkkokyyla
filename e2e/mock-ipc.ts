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
            return endedSessions
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
    })()
  })
}

declare global {
  interface Window {
    __TAURI_MOCK_SEND_PROBE__: (event: MockProbeEvent, sessionId?: number) => void
    __TAURI_MOCK_SEND_PROBES__: (events: MockProbeEvent[], sessionId?: number) => void
    __TAURI_MOCK_SEND_STATUS_ERROR__: (message: string, sessionId?: number) => void
    __TAURI_MOCK_SET_FALLBACK__: (enabled: boolean) => void
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
