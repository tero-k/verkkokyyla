import { Channel, invoke } from "@tauri-apps/api/core"
import type {
  DownloadProgressEvent,
  DownloadSpeedResultDto,
  Family,
  LoadedSessionDto,
  ProbeEvent,
  SessionSummaryDto,
  SnapshotDto,
  StartInfoDto,
  StatusEvent,
  StoppedSessionDto,
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

export function runDownloadSpeedTest(
  url: string,
  onProgress: (event: DownloadProgressEvent) => void,
): Promise<DownloadSpeedResultDto> {
  const onProgressChannel = new Channel<DownloadProgressEvent>(onProgress)
  return invoke<DownloadSpeedResultDto>("run_download_speed_test", {
    url,
    onProgress: onProgressChannel,
  })
}
