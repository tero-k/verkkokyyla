export const FAMILIES = ["auto", "v4", "v6"] as const
export type Family = (typeof FAMILIES)[number]

export const DEFAULT_PAYLOAD_SIZE = 32
export const MAX_PAYLOAD_SIZE = 65_507

export type ProbeEvent = {
  readonly seq: number
  readonly rttMs: number | null
  readonly lost: boolean
  readonly at: string
}

export type ProbeRow = {
  readonly seq: number
  readonly rttMs: number | null
  readonly lost: boolean
  readonly at: string
}

export type StatusEvent =
  | { readonly event: "engine-selected"; readonly engine: string; readonly fallback: boolean }
  | { readonly event: "error"; readonly message: string }
  | {
      readonly event: "session-stopped"
      readonly sessionId: number
      readonly probeCount: number
      readonly lossCount: number
    }

export type StartInfoDto = {
  readonly sessionId: number
  readonly engine: string
  readonly fallback: boolean
  readonly resolvedIp: string
  readonly answers: readonly string[]
  readonly payloadSize: number
  readonly dontFragment: boolean
}

export type SnapshotDto = {
  readonly count: number
  readonly lossCount: number
  readonly lossFraction: number
  readonly minMs: number | null
  readonly avgMs: number | null
  readonly maxMs: number | null
  readonly stddevMs: number | null
  readonly jitterMs: number | null
}

export type SessionSummaryDto = {
  readonly id: number
  readonly targetInput: string
  readonly resolvedIp: string
  readonly family: string
  readonly engine: string
  readonly intervalMs: number
  readonly timeoutMs: number
  readonly payloadSize: number
  readonly dontFragment: boolean
  readonly startedAt: string
  readonly endedAt: string | null
  readonly probeCount: number
  readonly lossCount: number
  readonly lossPercent: number
}

export type ProbeRowDto = {
  readonly seq: number
  readonly rttMs: number | null
  readonly loss: boolean
  readonly at: string
}

export type StoppedSessionDto = {
  readonly sessionId: number
  readonly probeCount: number
  readonly lossCount: number
  readonly endedAt: string
}

export type LoadedSessionDto = {
  readonly session: SessionSummaryDto
  readonly probes: readonly ProbeRowDto[]
}

export type DownloadProgressEvent = {
  readonly event: "progress"
  readonly bytesReceived: number
  readonly contentLength: number | null
  readonly elapsedMs: number
  readonly currentMbps: number
}

export type DownloadSpeedResultDto = {
  readonly url: string
  readonly finalUrl: string
  readonly statusCode: number
  readonly contentLength: number | null
  readonly bytesReceived: number
  readonly totalTimeMs: number
  readonly timeToFirstByteMs: number | null
  readonly dnsResolutionMs: number | null
  readonly tlsHandshakeMs: number | null
  readonly averageMbps: number
}

export type ValidationResult =
  | { readonly ok: true; readonly value: string }
  | { readonly ok: false; readonly error: string }
