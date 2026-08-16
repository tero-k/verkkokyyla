import type { TraceEvent, TraceHopRow } from "./types"

type HopEvent = Extract<TraceEvent, { readonly event: "hop" }>
type HostnameEvent = Extract<TraceEvent, { readonly event: "hostname" }>

function sortByHop(rows: readonly TraceHopRow[]): TraceHopRow[] {
  return [...rows].sort((left, right) => left.hop - right.hop)
}

function makeHopRow(event: HopEvent): TraceHopRow {
  return {
    hop: event.hop,
    address: event.address,
    hostname: null,
    rtt1Ms: event.rtt1Ms,
    rtt2Ms: event.rtt2Ms,
    rtt3Ms: event.rtt3Ms,
    annotation: event.annotation,
    at: event.at,
  }
}

export function applyHopEvent(rows: readonly TraceHopRow[], event: HopEvent): TraceHopRow[] {
  const index = rows.findIndex((row) => row.hop === event.hop)
  if (index === -1) {
    return sortByHop([...rows, makeHopRow(event)])
  }

  const next = [...rows]
  next[index] = {
    ...next[index],
    address: event.address,
    rtt1Ms: event.rtt1Ms,
    rtt2Ms: event.rtt2Ms,
    rtt3Ms: event.rtt3Ms,
    annotation: event.annotation,
    at: event.at,
  }
  return sortByHop(next)
}

export function applyHostnameEvent(
  rows: readonly TraceHopRow[],
  event: HostnameEvent,
): TraceHopRow[] {
  const index = rows.findIndex(
    (row) => row.hop === event.hop && row.address === event.address,
  )
  if (index === -1) {
    return rows.slice()
  }

  const next = [...rows]
  next[index] = {
    ...next[index],
    hostname: event.hostname,
  }
  return sortByHop(next)
}
