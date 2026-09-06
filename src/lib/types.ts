export const HTTP_VERSIONS = ["auto", "http1.1", "http2", "http3"] as const
export type HttpVersion = (typeof HTTP_VERSIONS)[number]

export const IP_FAMILIES = ["auto", "ipv4", "ipv6"] as const
export type IpFamily = (typeof IP_FAMILIES)[number]

export const CONNECTION_MODES = ["cold", "warm"] as const
export type ConnectionMode = (typeof CONNECTION_MODES)[number]

export const DEFAULT_HTTP_SETTINGS = {
  version: "auto" satisfies HttpVersion,
  connectTimeoutSec: 10,
  requestTimeoutSec: 60,
  readTimeoutSec: 0,
  followRedirects: true,
  maxRedirects: 10,
  compression: true,
  ipFamily: "auto" satisfies IpFamily,
  userAgent: "",
} as const

export type HttpSettings = {
  readonly version: HttpVersion
  readonly connectTimeoutSec: number
  readonly requestTimeoutSec: number
  readonly readTimeoutSec: number
  readonly followRedirects: boolean
  readonly maxRedirects: number
  readonly compression: boolean
  readonly ipFamily: IpFamily
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

export type TraceEvent =
  | {
      readonly event: "hop"
      readonly hop: number
      readonly address: string | null
      readonly rtt1Ms: number | null
      readonly rtt2Ms: number | null
      readonly rtt3Ms: number | null
      readonly annotation: string | null
      readonly at: string
    }
  | {
      readonly event: "hostname"
      readonly hop: number
      readonly address: string
      readonly hostname: string | null
    }

export type TraceStatusEvent =
  | {
      readonly event: "completed"
      readonly traceId: number
      readonly hopCount: number
      readonly reachedTarget: boolean
    }
  | {
      readonly event: "cancelled"
      readonly traceId: number
      readonly hopCount: number
    }
  | { readonly event: "error"; readonly message: string }

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

export type StartTraceDto = {
  readonly traceId: number
  readonly engine: string
  readonly resolvedIp: string
  readonly answers: readonly string[]
}

export type StoppedTraceDto = {
  readonly traceId: number
  readonly hopCount: number
  readonly endedAt: string
}

export type TraceSummaryDto = {
  readonly id: number
  readonly targetInput: string
  readonly resolvedIp: string
  readonly family: string
  readonly engine: string
  readonly maxHops: number
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly reachedTarget: boolean
  readonly hopCount: number
}

export type TraceHopDto = {
  readonly hop: number
  readonly address: string | null
  readonly hostname: string | null
  readonly rtt1Ms: number | null
  readonly rtt2Ms: number | null
  readonly rtt3Ms: number | null
  readonly annotation: string | null
  readonly at: string
}

export type TraceHopRow = TraceHopDto

export type ComparedHopRow = {
  readonly hop: number
  readonly a: TraceHopRow | null
  readonly b: TraceHopRow | null
  readonly status: "same" | "changed" | "a-only" | "b-only"
}

export type LoadedTraceDto = {
  readonly trace: TraceSummaryDto
  readonly hops: readonly TraceHopDto[]
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

export type InterfaceDto = {
  readonly name: string
  readonly description: string
  readonly ipv4: string
  readonly prefixLen: number
  readonly isPrimary: boolean
}

export interface OpenPort {
  readonly port: number
  readonly service: string
}

export type ScanEvent = {
  readonly event: "host"
  readonly ip: string
  readonly mac: string | null
  readonly vendor: string | null
  readonly hostname: string | null
  readonly foundBy: string
  readonly at: string
  readonly openPorts: readonly OpenPort[]
}

export type ScanStatusEvent =
  | { readonly event: "engine"; readonly engine: string; readonly tcpFallback: boolean }
  | { readonly event: "progress"; readonly done: number; readonly total: number }
  | { readonly event: "stopped"; readonly scanId: number; readonly hostCount: number }
  | { readonly event: "completed"; readonly scanId: number; readonly hostCount: number }
  | { readonly event: "error"; readonly message: string }

export type StartScanDto = {
  readonly scanId: number
  readonly interfaceName: string
  readonly cidr: string
  readonly tcpFallback: boolean
}

export type StoppedScanDto = {
  readonly scanId: number
  readonly hostCount: number
  readonly endedAt: string
}

export type ScanSummaryDto = {
  readonly id: number
  readonly interfaceName: string
  readonly cidr: string
  readonly tcpFallback: boolean
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly hostCount: number
}

export type ScanHostDto = {
  readonly ip: string
  readonly mac: string | null
  readonly vendor: string | null
  readonly hostname: string | null
  readonly foundBy: string
  readonly at: string
  readonly openPorts: readonly OpenPort[]
}

export type LoadedScanDto = {
  readonly scan: ScanSummaryDto
  readonly hosts: readonly ScanHostDto[]
}

// Download speed history types
export type DownloadSpeedSessionSummaryDto = {
  readonly id: number
  readonly url: string
  readonly mode: string
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly averageMbps: number
  readonly totalTimeMs: number
}

export type LoadedDownloadSpeedSessionDto = {
  readonly session: DownloadSpeedSessionSummaryDto
  readonly resultJson: string
}
export const DNS_PROTOCOLS = ["udp", "tcp", "tls", "https", "quic", "h3"] as const
export type DnsProtocol = (typeof DNS_PROTOCOLS)[number]

export type ResolverEndpointDto = {
  readonly name: string
  readonly address: string
  readonly protocol: DnsProtocol
}

export type AnswerDto = {
  readonly data: string
  readonly ttl: number
}

export type QueryResultDto = {
  readonly queryName: string
  readonly recordType: string
  readonly rcode: string
  readonly answers: readonly AnswerDto[]
  readonly authorityNodata: boolean
  readonly authoritySoa: boolean
  readonly adFlag: boolean
  readonly aaFlag: boolean
  readonly raFlag: boolean
  readonly truncated: boolean
  readonly ednsPresent: boolean
  readonly latencyMs: number
  readonly transportUsed: string
  readonly responseBytes: number
  readonly additionalGlue: readonly AnswerDto[]
}

export type LookupEventDto = {
  readonly queryName: string
  readonly recordType: string
  readonly result: QueryResultDto | { readonly kind: string; readonly message: string }
}

export type LookupSummaryDto = {
  readonly name: string
  readonly resolver: string
  readonly completed: number
  readonly failed: number
  readonly elapsedMs: number
}

export type EdnsSupportDto = {
  readonly optPresent: boolean
  readonly ednsVersion: number
  readonly responder: string
  readonly requestedVersion: number
  readonly headerRcode: number
  readonly extendedRcode: number
  readonly fullRcode: number
  readonly fullRcodeName: string
}

export type NameExistenceDto = {
  readonly name: string
  readonly exists: boolean
  readonly a: boolean
  readonly aaaa: boolean
  readonly mx: boolean
  readonly txt: boolean
  readonly cname: boolean
  readonly rcode: string
}

export type WildcardCheckDto = {
  readonly zone: string
  readonly probe: string
  readonly wildcard: boolean
  readonly rcode: string
  readonly answersCount: number
}

export type DnssecReportDto = {
  readonly domain: string
  readonly dnssecOk: boolean
  readonly adFlag: boolean
  readonly doBit: boolean
  readonly experimentalCaveat: string
}

export type DiagnosticStatus = "pass" | "info" | "inconclusive" | "warning" | "error"

export type DiagnosticEvidence = {
  readonly label: string
  readonly value: string
}

export type DiagnosticResultDto = {
  readonly id: string
  readonly title: string
  readonly status: DiagnosticStatus
  readonly summary: string
  readonly impact?: string
  readonly recommendation?: string
  readonly evidence: readonly DiagnosticEvidence[]
  readonly technicalDetails?: unknown
}

export type RecordInventoryItem = {
  readonly recordType: string
  readonly status: string
  readonly count?: number
}

export type ProbeSuccess<T> = {
  readonly data: T
  readonly server: string
  readonly rcode: string
  readonly aaFlag: boolean
}

export type ProbeFailure = {
  readonly server: string
  readonly error: string
  readonly rcode: string | null
}

export type ProbeResult<T> =
  | { readonly kind: "ok"; readonly value: ProbeSuccess<T> }
  | { readonly kind: "err"; readonly value: ProbeFailure }

export type DelegationReportDto = {
  readonly domain: string
  readonly parentNs: readonly string[]
  readonly parentNsError: string | null
  readonly childNs: readonly string[]
  readonly childNsError: string | null
  readonly nsConsistent: boolean | null
  readonly glueRecords: readonly string[]
  readonly authoritativeServers: readonly {
    readonly name: string
    readonly addresses: readonly string[]
    readonly nsQuery: ProbeResult<readonly string[]>
    readonly soaQuery: ProbeResult<number>
  }[]
  readonly authoritative: boolean
  readonly nsSerials: readonly {
    readonly name: string
    readonly address: string
    readonly serial: number | null
  }[]
  readonly serialsObserved: readonly number[]
  readonly serialConsistent: boolean | null
  readonly notes: readonly string[]
}

export type DnsDiagnosticsDto = {
  readonly domain: string
  readonly durationMs: number
  readonly overallStatus: DiagnosticStatus
  readonly summary: string
  readonly results: readonly DiagnosticResultDto[]
  readonly inventory: readonly RecordInventoryItem[]
  readonly technicalDetails: unknown
}

export type SampleOutcome = "Ok" | "Timeout" | "Servfail" | "Refused" | "Nxdomain" | "NoData"

export type Sample = {
  readonly latencyMs: number
  readonly outcome: SampleOutcome
  readonly coldConn: boolean
}

export type SampleCell = {
  readonly target: string
  readonly samples: readonly Sample[]
  readonly truncated: boolean
}

export type MetricsDto = {
  readonly target: string
  readonly count: number
  readonly min: number | null
  readonly median: number | null
  readonly mean: number | null
  readonly max: number | null
  readonly p90: number | null
  readonly p95: number | null
  readonly p99: number | null
  readonly successRate: number
  readonly timeoutRate: number
  readonly servfailRate: number
  readonly refusedRate: number
  readonly nxdomainRate: number
  readonly completedQps: number
}

export type BenchmarkRunDto = {
  readonly runId: number
  readonly profileName: string
  readonly endpointName: string
  readonly status: string
  readonly elapsedMs: number
  readonly metrics: MetricsDto
}

export type DnsRunSummaryDto = {
  readonly id: number
  readonly targetInput: string
  readonly kind: string
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly targetCount: number
}

export type DnsRunTargetDto = {
  readonly target: string
  readonly protocol: string | null
  readonly metricsJson: string
}

export type LoadedDnsRunDto = {
  readonly id: number
  readonly targetInput: string
  readonly kind: string
  readonly configJson: string
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly targets: readonly DnsRunTargetDto[]
}

export type QueryMix =
  | "popularWeighted"
  | { uniqueLabels: { base: string } }
  | { mixed: { ratio: number } }

export type BenchmarkProfile = {
  readonly name: string
  readonly concurrency: number
  readonly qpsLimit: number | null
  readonly durationSeconds: number
  readonly queryName: string
  readonly recordType: string
  readonly mix: QueryMix
  readonly cacheBust: boolean
}

export type BenchmarkPreset = "quick" | "stress" | "cache-bust"

// Email Security types
export type EmailSecurityVerdict = "pass" | "warn" | "fail"

export type SpfReportDto = {
  readonly verdict: EmailSecurityVerdict
  readonly record: string | null
  readonly recordCount: number
  readonly allMechanism: string | null
  readonly lookupCount: number
  readonly lookupLimitOk: boolean
  readonly notes: readonly string[]
}

export type DkimSelectorReportDto = {
  readonly selector: string
  readonly found: boolean
  readonly record: string | null
  readonly keyPresent: boolean
  readonly keyBitsApprox: number | null
  readonly revoked: boolean
  readonly verdict: EmailSecurityVerdict
  readonly notes: readonly string[]
}

export type DmarcReportDto = {
  readonly verdict: EmailSecurityVerdict
  readonly found: boolean
  readonly record: string | null
  readonly policy: string | null
  readonly subdomainPolicy: string | null
  readonly pct: number | null
  readonly reportingAddress: string | null
  readonly notes: readonly string[]
}

export type EmailSecurityReportDto = {
  readonly domain: string
  readonly spf: SpfReportDto
  readonly dkim: readonly DkimSelectorReportDto[]
  readonly dmarc: DmarcReportDto
  readonly elapsedMs: number
}

// Web Benchmark (HTTP diagnostics) types
export type WebBenchmarkConfig = {
  readonly url: string
  readonly protocols: readonly HttpVersion[]
  readonly runs: number
  readonly connectionMode: ConnectionMode
  readonly concurrency: number | null
  readonly probe: boolean
  readonly httpSettings: HttpSettings
}

export type WebBenchmarkConnectionProbe = {
  readonly dnsMs: number | null
  readonly connectMs: number | null
  readonly tlsMs: number | null
  readonly tlsVersion: string | null
  readonly alpn: string | null
  readonly sessionResumed: boolean | null
  readonly remoteIp: string | null
  readonly ipVersion: string | null
  readonly error: string | null
}

export type WebBenchmarkRun = {
  readonly runId: string
  readonly url: string
  readonly requestedProtocol: string
  readonly negotiatedProtocol: string | null
  readonly startedAt: string
  readonly statusCode: number | null
  readonly dnsMs: number | null
  readonly connectMs: number | null
  readonly tlsMs: number | null
  readonly ttfbMs: number | null
  readonly downloadMs: number | null
  readonly totalMs: number | null
  readonly responseBytes: number
  readonly decodedBytes: number | null
  readonly transferredBytes: number | null
  readonly throughputBytesPerSecond: number | null
  readonly remoteIp: string | null
  readonly ipVersion: string | null
  readonly tlsVersion: string | null
  readonly alpn: string | null
  readonly connectionReused: boolean | null
  readonly connectionMode: string
  readonly redirectCount: number
  readonly redirects: readonly string[]
  readonly finalUrl: string
  readonly contentEncoding: string | null
  readonly success: boolean
  readonly errorType: string | null
  readonly errorMessage: string | null
  readonly isWarmup: boolean
}

export type WebBenchmarkMetricSummary = {
  readonly count: number
  readonly min: number
  readonly max: number
  readonly average: number
  readonly p50: number
  readonly p90: number
  readonly p95: number
  readonly p99: number
  readonly stddev: number
}

export type WebBenchmarkProtocolSummary = {
  readonly requestedProtocol: string
  readonly negotiatedProtocol: string | null
  readonly successfulRuns: number
  readonly failedRuns: number
  readonly totalMs: WebBenchmarkMetricSummary
  readonly ttfbMs: WebBenchmarkMetricSummary
  readonly downloadMs: WebBenchmarkMetricSummary
  readonly dnsMs: WebBenchmarkMetricSummary
  readonly connectMs: WebBenchmarkMetricSummary
  readonly tlsMs: WebBenchmarkMetricSummary
  readonly responseBytes: WebBenchmarkMetricSummary
  readonly throughputBytesPerSecond: WebBenchmarkMetricSummary
  readonly probe: WebBenchmarkConnectionProbe | null
  readonly errorType: string | null
  readonly errorMessage: string | null
}

export type WebBenchmarkConcurrencySummary = {
  readonly concurrency: number
  readonly totalRequests: number
  readonly successfulRequests: number
  readonly failedRequests: number
  readonly requestsPerSecond: number
  readonly averageLatencyMs: number
  readonly p50LatencyMs: number
  readonly p95LatencyMs: number
  readonly p99LatencyMs: number
  readonly minLatencyMs: number
  readonly maxLatencyMs: number
  readonly totalBytes: number
  readonly aggregateThroughputBytesPerSecond: number
  readonly requestedProtocol: string
  readonly negotiatedProtocol: string | null
}

export type WebBenchmarkResult = {
  readonly benchmarkId: string
  readonly url: string
  readonly startedAt: string
  readonly config: WebBenchmarkConfig
  readonly runs: readonly WebBenchmarkRun[]
  readonly summaries: readonly WebBenchmarkProtocolSummary[]
  readonly concurrency: WebBenchmarkConcurrencySummary | null
  readonly concurrencyError: { readonly kind: string; readonly message: string } | null
}
