import { Channel, invoke } from "@tauri-apps/api/core"
import type {
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
  onProbe: (event: ProbeEvent) => void,
  onStatus: (event: StatusEvent) => void,
): Promise<StartInfoDto> {
  const onProbeChannel = new Channel<ProbeEvent>(onProbe)
  const onStatusChannel = new Channel<StatusEvent>(onStatus)
  return invoke<StartInfoDto>("start_session", {
    target,
    family,
    onProbe: onProbeChannel,
    onStatus: onStatusChannel,
  })
}

export function stopSession(): Promise<StoppedSessionDto> {
  return invoke<StoppedSessionDto>("stop_session")
}

export function getSnapshot(): Promise<SnapshotDto> {
  return invoke<SnapshotDto>("get_snapshot")
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
