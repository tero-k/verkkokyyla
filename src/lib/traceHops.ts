import type { ComparedHopRow, TraceEvent, TraceHopRow } from "./types"

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

function hopEqual(left: TraceHopRow, right: TraceHopRow): boolean {
  return (
    left.address === right.address &&
    left.hostname === right.hostname &&
    left.rtt1Ms === right.rtt1Ms &&
    left.rtt2Ms === right.rtt2Ms &&
    left.rtt3Ms === right.rtt3Ms &&
    left.annotation === right.annotation
  )
}

export function compareTraceHops(
  a: readonly TraceHopRow[],
  b: readonly TraceHopRow[],
): ComparedHopRow[] {
  const aSorted = sortByHop(a)
  const bSorted = sortByHop(b)
  const maxHop = Math.max(
    aSorted[aSorted.length - 1]?.hop ?? 0,
    bSorted[bSorted.length - 1]?.hop ?? 0,
  )

  const rows: ComparedHopRow[] = []
  for (let hop = 1; hop <= maxHop; hop++) {
    const aRow = aSorted.find((row) => row.hop === hop) ?? null
    const bRow = bSorted.find((row) => row.hop === hop) ?? null

    if (aRow === null && bRow === null) continue

    let status: ComparedHopRow["status"]
    if (aRow && bRow) {
      status = hopEqual(aRow, bRow) ? "same" : "changed"
    } else if (aRow) {
      status = "a-only"
    } else {
      status = "b-only"
    }

    rows.push({ hop, a: aRow, b: bRow, status })
  }
  return rows
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
