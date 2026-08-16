import { Channel, invoke } from "@tauri-apps/api/core"
import type {
  DownloadProgressEvent,
  DownloadSpeedResultDto,
  Family,
  HttpSettings,
  LoadedTraceDto,
  LoadedSessionDto,
  PageProgressEvent,
  PageSpeedResultDto,
  ProbeEvent,
  SessionSummaryDto,
  SnapshotDto,
  StartInfoDto,
  StartTraceDto,
  StatusEvent,
  StoppedTraceDto,
  StoppedSessionDto,
  TraceEvent,
  TraceStatusEvent,
  TraceSummaryDto,
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
