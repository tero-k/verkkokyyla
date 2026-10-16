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

export type UpdateInfo = {
  readonly version: string
  readonly url: string
  readonly current: string
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

export const MTU_METHODS = ["icmp", "tcp"] as const
export type MtuMethod = (typeof MTU_METHODS)[number]

export type ProbeOutcomeDto =
  | { readonly outcome: "ok"; readonly rttMs: number }
  | { readonly outcome: "too-big"; readonly hintMtu: number | null }
  | { readonly outcome: "timeout" }
  | { readonly outcome: "error"; readonly message: string }

export type MtuProbeEvent =
  | {
      readonly event: "attempt"
      readonly seq: number
      readonly payloadSize: number
      readonly mtuSize: number
    }
  | {
      readonly event: "outcome"
      readonly seq: number
      readonly outcome: ProbeOutcomeDto
    }

export type MtuLowerBoundReasonDto =
  | { readonly reason: "timeout-above"; readonly triedMtu: number }
  | { readonly reason: "ceiling-reached" }

export type ResultKindDto =
  | { readonly kind: "exact"; readonly mtu: number }
  | {
      readonly kind: "lower-bound"
      readonly mtu: number
      readonly reason: MtuLowerBoundReasonDto
    }
  | { readonly kind: "unreachable" }
  | { readonly kind: "failed"; readonly message: string }

export type MtuStatusEvent =
  | {
      readonly event: "completed"
      readonly runId: number
      readonly result: ResultKindDto
      readonly probesSent: number
    }
  | {
      readonly event: "cancelled"
      readonly runId: number
      readonly probesSent: number
    }
  | { readonly event: "error"; readonly message: string }

export type StartMtuDto = {
  readonly runId: number
  readonly method: string
  readonly resolvedIp: string
  readonly answers: readonly string[]
}

export type StoppedMtuDto = {
  readonly runId: number
  readonly probesSent: number
}

export type MtuRunSummaryDto = {
  readonly id: number
  readonly targetInput: string
  readonly resolvedIp: string
  readonly method: string
  readonly result: ResultKindDto
  readonly probesSent: number
  readonly startedAt: string
  readonly endedAt: string | null
}

export type MtuProbeDto = {
  readonly seq: number
  readonly payloadSize: number
  readonly mtuSize: number
  readonly outcome: ProbeOutcomeDto
  readonly at: string
}

export type LoadedMtuRunDto = {
  readonly run: MtuRunSummaryDto
  readonly probes: readonly MtuProbeDto[]
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

export type MikrotikResourcesDto = {
  readonly cpuLoad: number | null
  readonly memUsedBytes: number | null
  readonly memTotalBytes: number | null
  readonly uptime: string | null
  readonly boardName: string | null
  readonly routerosVersion: string | null
  readonly architectureName: string | null
}

export type MikrotikSensorDto = {
  readonly name: string
  readonly value: number
  readonly unit: string | null
  readonly kind: string
}

export type MikrotikInterfaceDto = {
  readonly name: string
  readonly type: string | null
  readonly running: boolean | null
  readonly disabled: boolean | null
  readonly rxByte: number | null
  readonly txByte: number | null
  readonly rxPacket: number | null
  readonly txPacket: number | null
  readonly txQueueDrop: number | null
  readonly linkDowns: number | null
  readonly rxError: number | null
  readonly txError: number | null
  readonly rxDrop: number | null
  readonly rxErrorEvents: number | null
  readonly txErrorEvents: number | null
  readonly rxFcsError: number | null
  readonly rxAlignError: number | null
  readonly txCollision: number | null
  readonly txDrop: number | null
  readonly rate: string | null
  readonly fullDuplex: boolean | null
  readonly comment: string | null
  readonly rxBitsPerSecond: number | null
  readonly txBitsPerSecond: number | null
}

export type MikrotikVlanDto = {
  readonly name: string
  readonly vlanId: number | null
  readonly interface: string | null
  readonly running: boolean | null
  readonly disabled: boolean | null
}

export type MikrotikBridgeVlanDto = {
  readonly bridge: string | null
  readonly vlanIds: readonly string[]
  readonly tagged: readonly string[]
  readonly untagged: readonly string[]
  readonly currentTagged: readonly string[]
  readonly currentUntagged: readonly string[]
}

export type MikrotikSnapshotEvent = {
  readonly event: "snapshot"
  readonly sessionId: number
  readonly at: string
  readonly resources: MikrotikResourcesDto | null
  readonly sensors: readonly MikrotikSensorDto[] | null
  readonly sensorsSupported: boolean
  readonly interfaces: readonly MikrotikInterfaceDto[]
  readonly vlans: readonly MikrotikVlanDto[] | null
  readonly bridgeVlans: readonly MikrotikBridgeVlanDto[] | null
  readonly warning: string | null
}

export type MikrotikUpdateStatus = {
  readonly installedVersion: string | null
  readonly latestVersion: string | null
  readonly channel: string | null
  readonly state: "update-available" | "up-to-date" | "unknown"
  readonly status: string
}

export type MikrotikFirmwareState = "available" | "up-to-date" | "not-applicable" | "unknown"

export type MikrotikFirmwareStatus = {
  readonly state: MikrotikFirmwareState
  readonly currentFirmware: string | null
  readonly upgradeFirmware: string | null
  readonly model: string | null
}

export type MikrotikStatusEvent =
  | { readonly event: "started"; readonly sessionId: number; readonly profileId: number }
  | { readonly event: "stopped"; readonly sessionId: number; readonly snapshotCount: number }
  | { readonly event: "cancelled"; readonly sessionId: number; readonly snapshotCount: number }
  | { readonly event: "warning"; readonly sessionId: number; readonly source: string; readonly message: string }
  | { readonly event: "error"; readonly sessionId: number; readonly message: string }
  | {
      readonly event: "version-firmware"
      readonly sessionId: number
      readonly updateStatus: MikrotikUpdateStatus
      readonly firmwareStatus: MikrotikFirmwareStatus
    }

export type MikrotikLogSeverity = "critical" | "error" | "warning" | "info" | "debug"

export type MikrotikLogEntry = {
  readonly id: string
  readonly time: string | null
  readonly topics: readonly string[]
  readonly message: string
  readonly severity: MikrotikLogSeverity
}

export type MikrotikLogEvent = {
  readonly event: "entries"
  readonly entries: readonly MikrotikLogEntry[]
}

export type MikrotikLogStatusEvent =
  | { readonly event: "started"; readonly profileId: number }
  | { readonly event: "stopped" }
  | { readonly event: "warning"; readonly message: string }
  | { readonly event: "error"; readonly message: string }

export type MikrotikLogStartDto = {
  readonly profileId: number
}

export type MikrotikProfile = {
  readonly id: number
  readonly name: string
  readonly host: string
  readonly port: number
  readonly useTls: boolean
  readonly allowInvalidCerts: boolean
  readonly username: string
  readonly hasPassword: boolean
  readonly createdAt: string
}

export type CreateMikrotikProfileRequest = {
  readonly name: string
  readonly host: string
  readonly port: number
  readonly useTls: boolean
  readonly allowInvalidCerts: boolean
  readonly username: string
}

export type UpdateMikrotikProfileRequest = CreateMikrotikProfileRequest & {
  readonly id: number
}

export type DeleteMikrotikProfileResultDto = {
  readonly deleted: boolean
  readonly secretDeleted: boolean
  readonly warning: string | null
}

export type MikrotikStartDto = {
  readonly sessionId: number
  readonly profileId: number
}

/** A freshly opened interactive SSH terminal, as returned by
    `mikrotik_terminal_open`. Output chunks flow over the command's data
    channel as base64 (the IPC layer is JSON; SSH bytes are not). */
export type MikrotikTerminalOpenDto = {
  readonly terminalId: number
  readonly profileId: number
}

/** One live monitoring session, as listed by `mikrotik_list_active`. Lets
    the UI rehydrate its device switcher after a reload. */
export type MikrotikActiveSessionDto = {
  readonly sessionId: number
  readonly profileId: number
}

export type MikrotikStoppedDto = {
  readonly sessionId: number
  readonly snapshotCount: number
  readonly endedAt: string
  readonly status: string
}

export type MikrotikTestConnectionDto = {
  readonly boardName: string | null
  readonly routerosVersion: string | null
  readonly architectureName: string | null
}

export type MikrotikSessionSummaryDto = {
  readonly id: number
  readonly profileId: number
  readonly startedAt: string
  readonly endedAt: string | null
  readonly status: string
  readonly boardName: string | null
  readonly routerosVersion: string | null
  readonly architectureName: string | null
  readonly updateStatusJson: string | null
  readonly firmwareStatusJson: string | null
  readonly snapshotCount: number
}

export type MikrotikSnapshotDto = {
  readonly id: number
  readonly sessionId: number
  readonly at: string
  readonly cpuLoad: number | null
  readonly memUsedBytes: number | null
  readonly memTotalBytes: number | null
  readonly uptime: string | null
  readonly boardName: string | null
  readonly routerosVersion: string | null
  readonly architectureName: string | null
  readonly warning: string | null
  readonly sensorsJson: string | null
  readonly interfacesJson: string | null
  readonly vlansJson: string | null
  readonly bridgeVlansJson: string | null
}

export type MikrotikLoadedSessionDto = {
  readonly session: MikrotikSessionSummaryDto
  readonly snapshots: readonly MikrotikSnapshotDto[]
}

export type MikrotikVersionFirmwareResultDto = {
  readonly updateStatus: MikrotikUpdateStatus
  readonly firmwareStatus: MikrotikFirmwareStatus
}

export type MikrotikChangelogDto = {
  readonly version: string
  readonly changelog: string
}

export type BackupResultDto = {
  readonly backupPath: string
  readonly exportPath: string | null
  readonly cleanupWarnings: readonly string[]
}

export type MikrotikBackupRecordDto = {
  readonly id: number
  readonly profileId: number | null
  readonly profileName: string
  readonly name: string
  readonly backupPath: string
  readonly exportPath: string | null
  readonly createdAt: string
  readonly sizeBytes: number
  readonly hasRscExport: boolean
}

export type DeleteMikrotikBackupResultDto = {
  readonly deleted: boolean
  readonly warnings: readonly string[]
}

export type BackupDiffLineKind = "same" | "add" | "remove"

export type BackupDiffLineDto = {
  readonly kind: BackupDiffLineKind
  readonly text: string
}

export type BackupDiffDto = {
  readonly olderId: number
  readonly olderName: string
  readonly olderCreatedAt: string
  readonly newerId: number
  readonly newerName: string
  readonly newerCreatedAt: string
  readonly lines: readonly BackupDiffLineDto[]
  readonly added: number
  readonly removed: number
}

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

// ---------------------------------------------------------------------------
// OPNsense — read-only monitoring. All device traffic is HTTP GET; the API
// key/secret never appears in any DTO (profiles serialize hasCredentials).
// ---------------------------------------------------------------------------

export type OpnsenseProfile = {
  readonly id: number
  readonly name: string
  readonly host: string
  readonly port: number
  readonly allowInvalidCerts: boolean
  readonly sshPort: number
  readonly hasCredentials: boolean
  readonly sshHostKeyVerified: boolean
  readonly createdAt: string
}

export type CreateOpnsenseProfileRequest = {
  readonly name: string
  readonly host: string
  readonly port: number
  readonly allowInvalidCerts: boolean
  readonly sshPort: number
}

export type UpdateOpnsenseProfileRequest = CreateOpnsenseProfileRequest & {
  readonly id: number
}

export type OpnsenseDeleteResultDto = {
  readonly deleted: boolean
  readonly secretDeleted: boolean
  readonly warning: string | null
}

/** `GET /api/core/system/status` probe: connected and authorized, without
    echoing any device response payload. */
export type OpnsenseTestConnectionDto = {
  readonly connected: boolean
  readonly authorized: boolean
}

export type OpnsenseStartDto = {
  readonly profileId: number
}

/** One entry of the active-monitor list: lets a reloaded view distinguish
    running-but-detached monitors. */
export type OpnsenseActiveMonitorDto = {
  readonly profileId: number
}

/** Typed manager/command error: serialized as `{kind, message}` plus
    `fingerprint` for host-key prompts. */
export type OpnsenseErrorPayload = {
  readonly kind: string
  readonly message: string
  readonly fingerprint?: string
}

/** Tauri command errors deserialize as `{kind, message}` objects, not Error
    instances — normalize whatever arrived into the typed payload. */
export function opnsenseErrorPayload(err: unknown): OpnsenseErrorPayload {
  if (typeof err === "object" && err !== null && "message" in err) {
    const kind: unknown = "kind" in err ? err.kind : undefined
    const fingerprint: unknown = "fingerprint" in err ? err.fingerprint : undefined
    const message: unknown = err.message
    return {
      kind: typeof kind === "string" ? kind : "error",
      message: String(message),
      ...(typeof fingerprint === "string" ? { fingerprint } : {}),
    }
  }
  if (err instanceof Error) return { kind: "error", message: err.message }
  return { kind: "error", message: String(err) }
}

export type OpnsenseSectionErrorDto = {
  readonly kind: string
  readonly message: string
}

/** One independently-fetched snapshot section: fresh `data`, or an error
    with no data (the view keeps showing the last good data, marked stale). */
export type OpnsenseSectionDto<T> = {
  readonly ok: boolean
  readonly error?: OpnsenseSectionErrorDto | null
  readonly data?: T | null
}

/** Lifecycle / failure events on the monitor status channel. */
export type OpnsenseStatusEvent =
  | { readonly event: "started"; readonly profileId: number }
  | { readonly event: "stopped"; readonly profileId: number }
  | { readonly event: "warning"; readonly profileId: number; readonly message: string }
  | { readonly event: "error"; readonly profileId: number; readonly message: string }

export type OpnsenseRuleDto = {
  readonly uuid: string
  readonly action: string
  readonly interface: string
  readonly description: string
  readonly source: string
  readonly destination: string
  readonly protocol: string
  readonly enabled: boolean
  /** "Hits (packets)" — null (rendered "—") when counters are missing. */
  readonly packets: number | null
  readonly evaluations: number | null
  readonly bytes: number | null
  readonly states: number | null
}

/** An active PF rule ID (or stats entry) matching no reported rule UUID.
    Numeric PF IDs are NEVER joined to UUIDs. */
export type OpnsenseUnmappedRuleDto = {
  readonly id: string
  readonly description?: string | null
  readonly packets?: number | null
  readonly bytes?: number | null
  readonly states?: number | null
}

export type OpnsenseRulesSectionDto = {
  readonly rules: readonly OpnsenseRuleDto[]
  /** Total rows the device reported (may exceed rules.length when truncated). */
  readonly total: number
  /** True when the 10,000-rule cap cut the listing short. */
  readonly truncated: boolean
  /** Present when rule definitions loaded but the counters endpoint failed. */
  readonly countersError?: OpnsenseSectionErrorDto | null
  readonly unmappedRunning: readonly OpnsenseUnmappedRuleDto[]
}

export type OpnsenseInterfaceDto = {
  readonly name: string
  readonly description?: string | null
  readonly enabled: boolean
  /** Link state as reported ("up"/"down"), null when not reported. */
  readonly link?: string | null
  /** All IPv4 addresses with CIDR prefix. */
  readonly ipv4: readonly string[]
  /** All IPv6 addresses with CIDR prefix. */
  readonly ipv6: readonly string[]
  readonly macaddr?: string | null
  readonly mtu?: number | null
  /** Link media/speed, e.g. "1000baseT <full-duplex>". */
  readonly media?: string | null
  /** Present on link-aggregation parents: protocol + member ports. */
  readonly lagg?: OpnsenseLaggDto | null
  readonly rxBytes?: number | null
  readonly txBytes?: number | null
  readonly rxPackets?: number | null
  readonly txPackets?: number | null
}

export type OpnsenseLaggDto = {
  /** "lacp" | "failover" | "loadbalance" | ... */
  readonly proto?: string | null
  readonly members: readonly OpnsenseLaggMemberDto[]
}

export type OpnsenseLaggMemberDto = {
  readonly name: string
  /** ifconfig laggport state flags, e.g. ["attached", "active"]. */
  readonly state: readonly string[]
  readonly link?: string | null
  readonly media?: string | null
}

/** PF state table capacity — a firewall health signal. */
export type OpnsensePfStatesDto = {
  readonly current: number | null
  readonly max: number | null
}

export type OpnsenseResourcesDto = {
  readonly cpuUsagePercent?: number | null
  readonly memUsedBytes?: number | null
  readonly memTotalBytes?: number | null
  readonly uptime?: string | null
  /** Load average string, e.g. "0.12, 0.08, 0.06". */
  readonly loadavg?: string | null
}

export type OpnsenseSystemSectionDto = {
  readonly interfaces: readonly OpnsenseInterfaceDto[]
  readonly interfacesTruncated: boolean
  readonly pfStates?: OpnsensePfStatesDto | null
  readonly pfStatesError?: OpnsenseSectionErrorDto | null
  readonly resources?: OpnsenseResourcesDto | null
  readonly resourcesError?: OpnsenseSectionErrorDto | null
}

/** One VPN service family, independent of the others: a missing plugin or
    a denied ACL must not break sibling cards. */
export type OpnsenseVpnServiceDto<T> = {
  /** "ok" | "unavailable" | "access-denied" | "error". */
  readonly state: string
  readonly message?: string | null
  readonly data?: T | null
}

export type OpnsenseWireguardPeerDto = {
  readonly name: string
  /** Seconds since the latest handshake; absent when never handshaked. */
  readonly handshakeAgeSecs?: number | null
  /** "active" | "stale" | "offline" | "never" — stale is DISTINCT from offline. */
  readonly status: string
  /** Remote endpoint "host:port". */
  readonly endpoint?: string | null
  readonly transferRxBytes?: number | null
  readonly transferTxBytes?: number | null
}

export type OpnsenseWireguardInterfaceDto = {
  readonly name: string
  readonly peers: readonly OpnsenseWireguardPeerDto[]
}

export type OpnsenseWireguardStatusDto = {
  readonly interfaces: readonly OpnsenseWireguardInterfaceDto[]
}

export type OpnsenseOpenvpnSessionDto = {
  readonly name: string
  readonly description?: string | null
  /** "client" | "server" | "unknown". */
  readonly kind: string
  readonly connected: boolean
  /** Client remote address "ip:port". */
  readonly realAddress?: string | null
  /** Client tunnel address. */
  readonly virtualAddress?: string | null
  readonly connectedSince?: string | null
  readonly bytesReceived?: number | null
  readonly bytesSent?: number | null
}

export type OpnsenseIpsecSessionDto = {
  readonly name: string
  readonly description?: string | null
  readonly connected: boolean
  readonly remoteHost?: string | null
  /** IKE version, e.g. "IKEv2". */
  readonly version?: string | null
}

/** Tailscale status with sensitive raw plugin JSON intentionally dropped. */
export type OpnsenseTailscaleStatusDto = {
  readonly backendState?: string | null
  readonly serviceRunning?: boolean | null
  readonly hostname?: string | null
  readonly ownIps: readonly string[]
  readonly peers: readonly OpnsenseTailscalePeerDto[]
}

export type OpnsenseTailscalePeerDto = {
  readonly name: string
  readonly ips: readonly string[]
  readonly online?: boolean | null
  readonly lastSeen?: string | null
}

export type OpnsenseVpnSectionDto = {
  readonly wireguard: OpnsenseVpnServiceDto<OpnsenseWireguardStatusDto>
  readonly openvpn: OpnsenseVpnServiceDto<readonly OpnsenseOpenvpnSessionDto[]>
  readonly ipsec: OpnsenseVpnServiceDto<readonly OpnsenseIpsecSessionDto[]>
  readonly tailscale: OpnsenseVpnServiceDto<OpnsenseTailscaleStatusDto>
}

/** Full monitoring snapshot streamed on the snapshot channel. */
export type OpnsenseSnapshotDto = {
  readonly profileId: number
  /** RFC 3339 UTC timestamp of this snapshot. */
  readonly at: string
  /** Only present on the 15 s rules refresh; omitted on 5 s system/VPN ticks. */
  readonly rules?: OpnsenseSectionDto<OpnsenseRulesSectionDto>
  readonly system: OpnsenseSectionDto<OpnsenseSystemSectionDto>
  readonly vpn: OpnsenseSectionDto<OpnsenseVpnSectionDto>
}

/** Installed firmware vs the firewall's cached release changelog. */
export type OpnsenseVersionCheckDto = {
  /** "up-to-date" | "update-available" | "unknown" | "error". */
  readonly state: string
  readonly message?: string | null
  /** Installed core version from the device. */
  readonly installedVersion?: string | null
  /** CORE_ABI as reported by the firewall, e.g. "26.7". */
  readonly abi?: string | null
  /** Release series, e.g. "25.7". */
  readonly series?: string | null
  /** Latest production version in the cached changelog for this series. */
  readonly latestVersion?: string | null
  /** RFC 3339 UTC timestamp when this app read the cache. */
  readonly checkedAt: string
  /** Cached changelog entries newer than the installed version, newest first. */
  readonly changelog: readonly OpnsenseChangelogEntryDto[]
  /** Installed plugins; cached upgrade targets may be stale. */
  readonly plugins: readonly OpnsensePluginDto[]
}

export type OpnsenseChangelogEntryDto = {
  readonly version: string
  readonly date?: string | null
}

export type OpnsensePluginDto = {
  readonly name: string
  readonly version?: string | null
  /** Target from the firewall's last cached update check, if reported. */
  readonly cachedUpgradeVersion?: string | null
}

export type OpnsenseRevisionDto = {
  readonly id: string
  readonly timeIso?: string | null
  readonly description?: string | null
  readonly filesize?: number | null
}

export type OpnsenseBackupResultDto = {
  /** Local path of the written `<name>.xml.age` file. */
  readonly path: string
  /** The saved firewall revision this backup came from — possibly older
      than unsaved changes on the device. */
  readonly sourceRevisionId: string
  readonly sourceTimeIso?: string | null
}

/** One normalized firewall log line. `action` is "pass" (allowed), "block"
    (blocked; reject maps here) or "other" (All tab only). Absent fields
    stay undefined — never fabricated. */
export type OpnsenseLogEvent = {
  readonly profileId: number
  /** Local receive time (RFC 3339 UTC). */
  readonly receivedAt: string
  readonly action: string
  readonly timestamp?: string | null
  readonly interfaceName?: string | null
  readonly dir?: string | null
  readonly proto?: string | null
  readonly src?: string | null
  readonly srcPort?: string | null
  readonly dst?: string | null
  readonly dstPort?: string | null
  readonly label?: string | null
}

/** Lifecycle / failure events for the firewall log stream. */
export type OpnsenseLogStatusEvent =
  | { readonly event: "started"; readonly profileId: number }
  | { readonly event: "stopped"; readonly profileId: number }
  | { readonly event: "warning"; readonly profileId: number; readonly message: string }
  | { readonly event: "error"; readonly profileId: number; readonly message: string }

export type OpnsenseLogStartDto = {
  readonly profileId: number
}

/** A freshly opened interactive SSH terminal. Output chunks flow over the
    command's data channel as base64 (the IPC layer is JSON). */
export type OpnsenseTerminalOpenDto = {
  readonly terminalId: string
}
