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
      let running = false
      let nextSessionId = 0
      let currentSessionId = 0
      let currentProbes: MockProbeEvent[] = []
      let currentTarget = ""
      let currentResolvedIp = ""
      let currentFamily = "auto"
      let currentEngine = "surge"
      let nextEngineIsFallback = false
      let currentPayloadSize = 32
      let currentDontFragment = false
      let sessions: MockSession[] = []
      let onProbeChannel: { onmessage?: (message: unknown) => void } | null = null
      let onStatusChannel: { onmessage?: (message: unknown) => void } | null = null

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
        const engine = nextEngineIsFallback ? "surge-fallback" : currentEngine
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

      function computeSnapshot(): Snapshot {
        const count = currentProbes.length
        const lossCount = currentProbes.filter((p) => p.lost).length
        const rtts = currentProbes
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

      async function invoke(
        cmd: string,
        args: Record<string, unknown>,
      ): Promise<unknown> {
        switch (cmd) {
          case "start_session": {
            if (running) {
              throw {
                kind: "already-running",
                message: "a ping session is already running",
              }
            }
            running = true
            currentTarget = String(args.target)
            currentFamily = String(args.family)
            currentPayloadSize =
              typeof args.payloadSize === "number" ? args.payloadSize : 32
            currentDontFragment =
              typeof args.dontFragment === "boolean" ? args.dontFragment : false
            currentProbes = []
            const info = resolveInfo(currentTarget)
            currentResolvedIp = info.resolvedIp
            currentEngine = info.engine
            nextSessionId += 1
            currentSessionId = nextSessionId
            onProbeChannel = args.onProbe as typeof onProbeChannel
            onStatusChannel = args.onStatus as typeof onStatusChannel
            sendChannel(onStatusChannel, {
              event: "engine-selected",
              engine: currentEngine,
              fallback: info.fallback,
            })
            return {
              sessionId: currentSessionId,
              engine: currentEngine,
              fallback: info.fallback,
              resolvedIp: currentResolvedIp,
              answers: [currentResolvedIp],
              payloadSize: currentPayloadSize,
              dontFragment: currentDontFragment,
            }
          }
          case "stop_session": {
            if (!running) {
              throw {
                kind: "not-running",
                message: "no ping session is running",
              }
            }
            if (args.sessionId !== currentSessionId) {
              throw {
                kind: "not-running",
                message: "session not found",
              }
            }
            running = false
            const snap = computeSnapshot()
            const endedAt = new Date().toISOString()
            const startedAt = new Date(
              Date.now() - snap.count * 1000,
            ).toISOString()
            sessions.push({
              id: currentSessionId,
              targetInput: currentTarget,
              resolvedIp: currentResolvedIp,
              family: currentFamily,
              engine: currentEngine,
              intervalMs: 1000,
              timeoutMs: 1000,
              payloadSize: currentPayloadSize,
              dontFragment: currentDontFragment,
              startedAt,
              endedAt,
              probeCount: snap.count,
              lossCount: snap.lossCount,
              lossPercent: snap.lossFraction * 100,
            })
            sendChannel(onStatusChannel, {
              event: "session-stopped",
              sessionId: currentSessionId,
              probeCount: snap.count,
              lossCount: snap.lossCount,
            })
            return {
              sessionId: currentSessionId,
              probeCount: snap.count,
              lossCount: snap.lossCount,
              endedAt,
            }
          }
          case "get_snapshot":
            return computeSnapshot()
          case "list_active_sessions":
            return running ? [currentSessionId] : []
          case "list_sessions":
            return sessions
          case "load_session": {
            const session = sessions.find((s) => s.id === args.id)
            if (session === undefined) {
              throw {
                kind: "session-not-found",
                message: `no session with id ${args.id}`,
              }
            }
            const probes = currentProbes.map((p) => ({
              seq: p.seq,
              rttMs: p.rttMs,
              loss: p.lost,
              at: p.at,
            }))
            return { session, probes }
          }
          case "delete_session": {
            sessions = sessions.filter((s) => s.id !== args.id)
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

      window.__TAURI_MOCK_SEND_PROBE__ = (event) => {
        if (!running) return
        currentProbes.push(event)
        sendChannel(onProbeChannel, event)
      }

      window.__TAURI_MOCK_SEND_PROBES__ = (events) => {
        if (!running) return
        for (const event of events) {
          currentProbes.push(event)
          sendChannel(onProbeChannel, event)
        }
      }

      window.__TAURI_MOCK_SEND_STATUS_ERROR__ = (message) => {
        sendChannel(onStatusChannel, { event: "error", message })
      }

      window.__TAURI_MOCK_SET_FALLBACK__ = (enabled) => {
        nextEngineIsFallback = enabled
      }
    })()
  })
}

declare global {
  interface Window {
    __TAURI_MOCK_SEND_STATUS_ERROR__: (message: string) => void
    __TAURI_MOCK_SET_FALLBACK__: (enabled: boolean) => void
  }
}

export function makeProbe(
  seq: number,
  rtt: number | null,
  lost: boolean,
): MockProbeEvent {
  return { seq, rttMs: rtt, lost, at: new Date(seq * 1000).toISOString() }
}
