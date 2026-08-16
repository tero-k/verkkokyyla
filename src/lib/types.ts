export const HTTP_VERSIONS = ["auto", "http1.1", "http2"] as const
export type HttpVersion = (typeof HTTP_VERSIONS)[number]

export const DEFAULT_HTTP_SETTINGS = {
  version: "auto" satisfies HttpVersion,
  connectTimeoutSec: 10,
  requestTimeoutSec: 60,
  followRedirects: true,
  maxRedirects: 10,
  compression: true,
  userAgent: "",
} as const

export type HttpSettings = {
  readonly version: HttpVersion
  readonly connectTimeoutSec: number
  readonly requestTimeoutSec: number
  readonly followRedirects: boolean
  readonly maxRedirects: number
  readonly compression: boolean
  readonly userAgent: string
}

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

export const PAGE_RESOURCE_TYPES = [
  "document",
  "stylesheet",
  "script",
  "image",
  "font",
  "xhr",
  "other",
] as const
export type PageResourceType = (typeof PAGE_RESOURCE_TYPES)[number]

export type PageResourceDto = {
  readonly url: string
  readonly resourceType: PageResourceType
  readonly statusCode: number | null
  readonly contentLength: number | null
  readonly bytesReceived: number
  readonly startOffsetMs: number
  readonly durationMs: number
  readonly timeToFirstByteMs: number | null
  readonly averageMbps: number
  readonly error: string | null
}

export type PageProgressEvent = {
  readonly event: "progress"
  readonly resource: PageResourceDto
  readonly completed: number
  readonly total: number
}

export type PageSpeedResultDto = {
  readonly url: string
  readonly totalResources: number
  readonly successfulResources: number
  readonly failedResources: number
  readonly totalBytesReceived: number
  readonly totalDurationMs: number
  readonly timeToFirstByteMs: number | null
  readonly averageMbps: number
  readonly resources: readonly PageResourceDto[]
}

export type ValidationResult =
  | { readonly ok: true; readonly value: string }
  | { readonly ok: false; readonly error: string }
