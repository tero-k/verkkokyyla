import { Channel, invoke } from "@tauri-apps/api/core"
import type {
  BenchmarkProfile,
  BenchmarkRunDto,
  BackupResultDto,
  BackupDiffDto,  CreateMikrotikProfileRequest,
  DeleteMikrotikBackupResultDto,
  DeleteMikrotikProfileResultDto,
  DnsDiagnosticsDto,
  DnsRunSummaryDto,
  DownloadProgressEvent,
  DownloadSpeedResultDto,
  EmailSecurityReportDto,
  Family,
  HttpSettings,
  InterfaceDto,
  LoadedDnsRunDto,
  LoadedMtuRunDto,
  LoadedScanDto,
  LoadedDownloadSpeedSessionDto,
  DownloadSpeedSessionSummaryDto,
  LoadedSessionDto,
  LoadedTraceDto,
  LookupEventDto,
  LookupSummaryDto,
  MikrotikChangelogDto,
  MikrotikBackupRecordDto,
  MikrotikLoadedSessionDto,
  MikrotikLogEvent,
  MikrotikActiveSessionDto,
  MikrotikLogStartDto,
  MikrotikLogStatusEvent,
  MikrotikProfile,
  MikrotikSnapshotEvent,
  MikrotikSessionSummaryDto,
  MikrotikStartDto,
  MikrotikStatusEvent,
  MikrotikStoppedDto,
  MikrotikTerminalOpenDto,
  MikrotikTestConnectionDto,
  MikrotikVersionFirmwareResultDto,
  MtuMethod,
  MtuProbeEvent,
  MtuRunSummaryDto,
  MtuStatusEvent,
  PageProgressEvent,
  PageSpeedResultDto,
  ProbeEvent,
  ResolverEndpointDto,
  SampleCell,
  ScanEvent,
  ScanStatusEvent,
  ScanSummaryDto,
  SessionSummaryDto,
  SnapshotDto,
  StartInfoDto,
  StartMtuDto,
  StartScanDto,
  StartTraceDto,
  StatusEvent,
  StoppedScanDto,
  StoppedMtuDto,
  StoppedSessionDto,
  StoppedTraceDto,
  TraceEvent,
  TraceStatusEvent,
  TraceSummaryDto,
  UpdateMikrotikProfileRequest,
  WebBenchmarkConfig,
  WebBenchmarkResult,
} from "./types"

export function startSession(
  target: string,
  family: Family,
  payloadSize: number,
  dontFragment: boolean,
  onProbe: (event: ProbeEvent) => void,
  onStatus: (event: StatusEvent) => void,
): Promise<StartInfoDto> {
  const onProbeChannel = new Channel<ProbeEvent>(onProbe)
  const onStatusChannel = new Channel<StatusEvent>(onStatus)
  return invoke<StartInfoDto>("start_session", {
    target,
    family,
    payloadSize,
    dontFragment,
    onProbe: onProbeChannel,
    onStatus: onStatusChannel,
  })
}

export function stopSession(sessionId: number): Promise<StoppedSessionDto> {
  return invoke<StoppedSessionDto>("stop_session", { sessionId })
}

export function getSnapshot(sessionId: number): Promise<SnapshotDto> {
  return invoke<SnapshotDto>("get_snapshot", { sessionId })
}

export function listActiveSessions(): Promise<number[]> {
  return invoke<number[]>("list_active_sessions")
}

export function listSessions(): Promise<SessionSummaryDto[]> {
  return invoke<SessionSummaryDto[]>("list_sessions")
}

export function loadSession(id: number): Promise<LoadedSessionDto> {
  return invoke<LoadedSessionDto>("load_session", { id })
}

export function deleteSession(id: number): Promise<void> {
  return invoke<void>("delete_session", { id })
}

export function startTrace(
  target: string,
  family: Family,
  onEvent: (event: TraceEvent) => void,
  onStatus: (event: TraceStatusEvent) => void,
): Promise<StartTraceDto> {
  const onEventChannel = new Channel<TraceEvent>(onEvent)
  const onStatusChannel = new Channel<TraceStatusEvent>(onStatus)
  return invoke<StartTraceDto>("start_trace", {
    target,
    family,
    onEvent: onEventChannel,
    onStatus: onStatusChannel,
  })
}

export function stopTrace(): Promise<StoppedTraceDto> {
  return invoke<StoppedTraceDto>("stop_trace")
}

export function listTraces(): Promise<TraceSummaryDto[]> {
  return invoke<TraceSummaryDto[]>("list_traces")
}

export function loadTrace(id: number): Promise<LoadedTraceDto> {
  return invoke<LoadedTraceDto>("load_trace", { id })
}

export function deleteTrace(id: number): Promise<void> {
  return invoke<void>("delete_trace", { id })
}

export function startMtuProbe(
  target: string,
  method: MtuMethod,
  ceilingMtu: number,
  port: number,
  onEvent: (event: MtuProbeEvent) => void,
  onStatus: (event: MtuStatusEvent) => void,
): Promise<StartMtuDto> {
  const onEventChannel = new Channel<MtuProbeEvent>(onEvent)
  const onStatusChannel = new Channel<MtuStatusEvent>(onStatus)
  return invoke<StartMtuDto>("start_mtu_probe", {
    target,
    method,
    ceilingMtu,
    port,
    onEvent: onEventChannel,
    onStatus: onStatusChannel,
  })
}

export function stopMtuProbe(): Promise<StoppedMtuDto> {
  return invoke<StoppedMtuDto>("stop_mtu_probe")
}

export function listMtuRuns(): Promise<MtuRunSummaryDto[]> {
  return invoke<MtuRunSummaryDto[]>("list_mtu_runs")
}

export function loadMtuRun(id: number): Promise<LoadedMtuRunDto> {
  return invoke<LoadedMtuRunDto>("load_mtu_run", { id })
}

export function deleteMtuRun(id: number): Promise<void> {
  return invoke<void>("delete_mtu_run", { id })
}

export function runDownloadSpeedTest(
  url: string,
  settings: HttpSettings,
  onProgress: (event: DownloadProgressEvent) => void,
): Promise<DownloadSpeedResultDto> {
  const onProgressChannel = new Channel<DownloadProgressEvent>(onProgress)
  return invoke<DownloadSpeedResultDto>("run_download_speed_test", {
    url,
    settings,
    onProgress: onProgressChannel,
  })
}

export function runPageSpeedTest(
  url: string,
  settings: HttpSettings,
  onProgress: (event: PageProgressEvent) => void,
): Promise<PageSpeedResultDto> {
  const onProgressChannel = new Channel<PageProgressEvent>(onProgress)
  return invoke<PageSpeedResultDto>("run_page_speed_test", {
    url,
    settings,
    onProgress: onProgressChannel,
  })
}

export function runWebBenchmark(
  config: WebBenchmarkConfig,
): Promise<WebBenchmarkResult> {
  return invoke<WebBenchmarkResult>("run_web_benchmark", { config })
}

export function listInterfaces(): Promise<InterfaceDto[]> {
  return invoke<InterfaceDto[]>("list_interfaces")
}

export function startScan(
  interfaceName: string,
  cidr: string,
  tcpFallback: boolean,
  portsEnabled: boolean,
  onEvent: (event: ScanEvent) => void,
  onStatus: (event: ScanStatusEvent) => void,
): Promise<StartScanDto> {
  const onEventChannel = new Channel<ScanEvent>(onEvent)
  const onStatusChannel = new Channel<ScanStatusEvent>(onStatus)
  return invoke<StartScanDto>("start_scan", {
    interfaceName,
    cidr,
    tcpFallback,
    portsEnabled,
    onEvent: onEventChannel,
    onStatus: onStatusChannel,
  })
}

export function stopScan(): Promise<StoppedScanDto> {
  return invoke<StoppedScanDto>("stop_scan")
}

export function listScans(): Promise<ScanSummaryDto[]> {
  return invoke<ScanSummaryDto[]>("list_scans")
}

export function loadScan(id: number): Promise<LoadedScanDto> {
  return invoke<LoadedScanDto>("load_scan", { id })
}

export function deleteScan(id: number): Promise<void> {
  return invoke<void>("delete_scan", { id })
}

// MikroTik IPC wrappers

export function mikrotikListProfiles(): Promise<MikrotikProfile[]> {
  return invoke<MikrotikProfile[]>("mikrotik_list_profiles")
}

export function mikrotikCreateProfile(
  request: CreateMikrotikProfileRequest,
): Promise<MikrotikProfile> {
  return invoke<MikrotikProfile>("mikrotik_create_profile", { request })
}

export function mikrotikUpdateProfile(
  request: UpdateMikrotikProfileRequest,
): Promise<MikrotikProfile> {
  return invoke<MikrotikProfile>("mikrotik_update_profile", { request })
}

export function mikrotikDeleteProfile(id: number): Promise<DeleteMikrotikProfileResultDto> {
  return invoke<DeleteMikrotikProfileResultDto>("mikrotik_delete_profile", { id })
}

export function mikrotikSetProfilePassword(id: number, password: string): Promise<void> {
  return invoke<void>("mikrotik_set_profile_password", { id, password })
}

export function mikrotikTestConnection(id: number): Promise<MikrotikTestConnectionDto> {
  return invoke<MikrotikTestConnectionDto>("mikrotik_test_connection", { id })
}

export function mikrotikStart(
  profileId: number,
  onEvent: (event: MikrotikSnapshotEvent) => void,
  onStatus: (event: MikrotikStatusEvent) => void,
): Promise<MikrotikStartDto> {
  const onEventChannel = new Channel<MikrotikSnapshotEvent>(onEvent)
  const onStatusChannel = new Channel<MikrotikStatusEvent>(onStatus)
  return invoke<MikrotikStartDto>("mikrotik_start", {
    profileId,
    onEvent: onEventChannel,
    onStatus: onStatusChannel,
  })
}

export function mikrotikStop(sessionId: number): Promise<MikrotikStoppedDto> {
  return invoke<MikrotikStoppedDto>("mikrotik_stop", { sessionId })
}

export function mikrotikListActive(): Promise<MikrotikActiveSessionDto[]> {
  return invoke<MikrotikActiveSessionDto[]>("mikrotik_list_active")
}

export function mikrotikTerminalOpen(
  profileId: number,
  cols: number,
  rows: number,
  onData: (chunk: string) => void,
): Promise<MikrotikTerminalOpenDto> {
  const onDataChannel = new Channel<string>(onData)
  return invoke<MikrotikTerminalOpenDto>("mikrotik_terminal_open", {
    profileId,
    cols,
    rows,
    onData: onDataChannel,
  })
}

export function mikrotikTerminalWrite(terminalId: number, data: string): Promise<void> {
  return invoke<void>("mikrotik_terminal_write", { terminalId, data })
}

export function mikrotikTerminalResize(
  terminalId: number,
  cols: number,
  rows: number,
): Promise<void> {
  return invoke<void>("mikrotik_terminal_resize", { terminalId, cols, rows })
}

export function mikrotikTerminalClose(terminalId: number): Promise<void> {
  return invoke<void>("mikrotik_terminal_close", { terminalId })
}

export function mikrotikLogStart(
  profileId: number,
  pollSeconds: number,
  onEvent: (event: MikrotikLogEvent) => void,
  onStatus: (event: MikrotikLogStatusEvent) => void,
): Promise<MikrotikLogStartDto> {
  const onEventChannel = new Channel<MikrotikLogEvent>(onEvent)
  const onStatusChannel = new Channel<MikrotikLogStatusEvent>(onStatus)
  return invoke<MikrotikLogStartDto>("mikrotik_log_start", {
    profileId,
    pollSeconds,
    onEvent: onEventChannel,
    onStatus: onStatusChannel,
  })
}

export function mikrotikLogStop(profileId: number): Promise<void> {
  return invoke<void>("mikrotik_log_stop", { profileId })
}

export function mikrotikListSessions(): Promise<MikrotikSessionSummaryDto[]> {
  return invoke<MikrotikSessionSummaryDto[]>("mikrotik_list_sessions")
}

export function mikrotikLoadSession(id: number): Promise<MikrotikLoadedSessionDto> {
  return invoke<MikrotikLoadedSessionDto>("mikrotik_load_session", { id })
}

export function mikrotikDeleteSession(id: number): Promise<void> {
  return invoke<void>("mikrotik_delete_session", { id })
}

export function mikrotikCheckUpdates(profileId: number): Promise<MikrotikVersionFirmwareResultDto> {
  return invoke<MikrotikVersionFirmwareResultDto>("mikrotik_check_updates", { profileId })
}

export function mikrotikFetchChangelog(version: string): Promise<MikrotikChangelogDto> {
  return invoke<MikrotikChangelogDto>("mikrotik_changelog", { version })
}

export function mikrotikBackup(
  profileId: number,
  destinationDir: string,
  backupName: string,
  password: string | undefined,
  includeRsc: boolean,
  overwrite: boolean,
): Promise<BackupResultDto> {
  return invoke<BackupResultDto>("mikrotik_backup", {
    profileId,
    destinationDir,
    backupName,
    password,
    includeRsc,
    overwrite,
  })
}

export function mikrotikListBackups(): Promise<MikrotikBackupRecordDto[]> {
  return invoke<MikrotikBackupRecordDto[]>("mikrotik_list_backups")
}

export function mikrotikDeleteBackup(id: number): Promise<DeleteMikrotikBackupResultDto> {
  return invoke<DeleteMikrotikBackupResultDto>("mikrotik_delete_backup", { id })
}

export function mikrotikGetBackupDestination(): Promise<string | null> {
  return invoke<string | null>("mikrotik_get_backup_destination")
}

export function mikrotikSetBackupDestination(path: string): Promise<void> {
  return invoke<void>("mikrotik_set_backup_destination", { path })
}

export function mikrotikDiffBackups(olderId: number, newerId: number): Promise<BackupDiffDto> {
  return invoke<BackupDiffDto>("mikrotik_diff_backups", { olderId, newerId })
}

// DNS Tester IPC wrappers

export function runDnsLookup(
  name: string,
  recordTypes: string[],
  endpoint: ResolverEndpointDto,
  onEvent: (event: LookupEventDto) => void,
): Promise<LookupSummaryDto> {
  const onEventChannel = new Channel<LookupEventDto>(onEvent)
  return invoke<LookupSummaryDto>("dns_lookup", {
    name,
    recordTypes,
    endpoint,
    onEvent: onEventChannel,
  })
}

export function runDnsDiagnostics(
  endpoint: ResolverEndpointDto,
  domain: string,
): Promise<DnsDiagnosticsDto> {
  return invoke<DnsDiagnosticsDto>("dns_diagnostics", { endpoint, domain })
}

export function runDnsEmailCheck(
  endpoint: ResolverEndpointDto,
  domain: string,
  dkimSelectors: string[],
): Promise<EmailSecurityReportDto> {
  return invoke<EmailSecurityReportDto>("dns_email_check", { endpoint, domain, dkimSelectors })
}

export function runDnsBenchmark(
  endpoint: ResolverEndpointDto,
  profile: BenchmarkProfile,
  onCell: (cell: SampleCell) => void,
): Promise<BenchmarkRunDto> {
  const onCellChannel = new Channel<SampleCell>(onCell)
  return invoke<BenchmarkRunDto>("dns_benchmark", {
    endpoint,
    profileJson: JSON.stringify(profile),
    onCell: onCellChannel,
  })
}

export function listDnsRuns(): Promise<DnsRunSummaryDto[]> {
  return invoke<DnsRunSummaryDto[]>("list_dns_runs")
}

export function loadDnsRun(id: number): Promise<LoadedDnsRunDto> {
  return invoke<LoadedDnsRunDto>("load_dns_run", { id })
}

export function deleteDnsRun(id: number): Promise<void> {
  return invoke<void>("delete_dns_run", { id })
}

// Download speed history IPC wrappers

type SpeedSessionResult =
  | DownloadSpeedResultDto
  | PageSpeedResultDto
  | WebBenchmarkResult

function sessionMetrics(result: SpeedSessionResult): {
  averageMbps: number
  totalTimeMs: number
} {
  if ("averageMbps" in result) {
    return {
      averageMbps: result.averageMbps,
      totalTimeMs:
        "totalTimeMs" in result
          ? (result as DownloadSpeedResultDto).totalTimeMs
          : (result as PageSpeedResultDto).totalDurationMs,
    }
  }

  const summary = result.summaries[0]
  if (!summary) {
    return { averageMbps: 0, totalTimeMs: 0 }
  }
  const maxTotalMs = result.summaries.reduce(
    (max, s) => Math.max(max, s.totalMs.average),
    0,
  )
  return {
    averageMbps: (summary.throughputBytesPerSecond.average * 8) / 1_000_000,
    totalTimeMs: Math.round(maxTotalMs),
  }
}

export function saveDownloadSpeedSession(
  url: string,
  mode: string,
  httpSettings: HttpSettings,
  result: SpeedSessionResult,
): Promise<DownloadSpeedSessionSummaryDto> {
  const metrics = sessionMetrics(result)
  return invoke<DownloadSpeedSessionSummaryDto>("save_download_speed_session", {
    request: {
      url,
      mode,
      httpSettingsJson: JSON.stringify(httpSettings),
      resultJson: JSON.stringify(result),
      averageMbps: metrics.averageMbps,
      totalTimeMs: metrics.totalTimeMs,
    },
  })
}

export function listDownloadSpeedSessions(): Promise<DownloadSpeedSessionSummaryDto[]> {
  return invoke<DownloadSpeedSessionSummaryDto[]>("list_download_speed_sessions")
}

export function loadDownloadSpeedSession(id: number): Promise<LoadedDownloadSpeedSessionDto> {
  return invoke<LoadedDownloadSpeedSessionDto>("load_download_speed_session", { id })
}

export function deleteDownloadSpeedSession(id: number): Promise<void> {
  return invoke<void>("delete_download_speed_session", { id })
}
