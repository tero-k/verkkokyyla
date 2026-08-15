import type { Page } from "@playwright/test"

export type MockProbeEvent = {
  seq: number
  rttMs: number | null
  lost: boolean
  at: string
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
      if (
        typeof window !== "undefined" &&
        Array.isArray(window.__TAURI_MOCK_ENDED_SESSIONS__)
      ) {
        endedSessions.push(...window.__TAURI_MOCK_ENDED_SESSIONS__)
      }
      let nextEngineIsFallback = false

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
  }
}

export function makeProbe(
  seq: number,
  rtt: number | null,
  lost: boolean,
): MockProbeEvent {
  return { seq, rttMs: rtt, lost, at: new Date(seq * 1000).toISOString() }
}
