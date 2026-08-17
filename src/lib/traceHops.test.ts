import { describe, expect, it } from "vitest"
import { applyHostnameEvent, applyHopEvent, compareTraceHops } from "./traceHops"
import type { TraceHopRow } from "./types"

function makeRow(hop: number, address: string | null, hostname: string | null): TraceHopRow {
  return {
    hop,
    address,
    hostname,
    rtt1Ms: hop + 1,
    rtt2Ms: hop + 2,
    rtt3Ms: hop + 3,
    annotation: null,
    at: new Date(hop * 1000).toISOString(),
  }
}

describe("applyHopEvent", () => {
  it("upserts a hop row and keeps rows sorted by hop number", () => {
    const rows = [makeRow(3, "10.0.0.3", null), makeRow(1, "10.0.0.1", "old-name")]
    const result = applyHopEvent(rows, {
      event: "hop",
      hop: 1,
      address: "10.0.0.1",
      rtt1Ms: 11,
      rtt2Ms: 12,
      rtt3Ms: 13,
      annotation: "!H",
      at: "2026-08-16T00:00:01.000Z",
    })

    expect(result).toHaveLength(2)
    expect(result.map((row) => row.hop)).toEqual([1, 3])
    expect(result[0]).toEqual({
      hop: 1,
      address: "10.0.0.1",
      hostname: "old-name",
      rtt1Ms: 11,
      rtt2Ms: 12,
      rtt3Ms: 13,
      annotation: "!H",
      at: "2026-08-16T00:00:01.000Z",
    })
  })

  it("stores an all-star hop row with a null address", () => {
    const result = applyHopEvent([], {
      event: "hop",
      hop: 2,
      address: null,
      rtt1Ms: null,
      rtt2Ms: null,
      rtt3Ms: null,
      annotation: null,
      at: "2026-08-16T00:00:02.000Z",
    })

    expect(result).toEqual([
      {
        hop: 2,
        address: null,
        hostname: null,
        rtt1Ms: null,
        rtt2Ms: null,
        rtt3Ms: null,
        annotation: null,
        at: "2026-08-16T00:00:02.000Z",
      },
    ])
  })
})

describe("applyHostnameEvent", () => {
  it("updates the matching hop row", () => {
    const rows = [makeRow(1, "10.0.0.1", null), makeRow(2, "10.0.0.2", null)]
    const result = applyHostnameEvent(rows, {
      event: "hostname",
      hop: 2,
      address: "10.0.0.2",
      hostname: "router.example.net",
    })

    expect(result[1].hostname).toBe("router.example.net")
    expect(result[0].hostname).toBeNull()
  })

  it("ignores hostname events for unknown hops", () => {
    const rows = [makeRow(1, "10.0.0.1", null)]
    const result = applyHostnameEvent(rows, {
      event: "hostname",
      hop: 99,
      address: "10.0.0.99",
      hostname: "missing.example.net",
    })

    expect(result).toEqual(rows)
  })

  it("applies a late hostname after the trace has completed", () => {
    const completedRows = [
      applyHopEvent([], {
        event: "hop",
        hop: 1,
        address: "10.0.0.1",
        rtt1Ms: 7,
        rtt2Ms: 8,
        rtt3Ms: 9,
        annotation: null,
        at: "2026-08-16T00:00:01.000Z",
      })[0],
      applyHopEvent([], {
        event: "hop",
        hop: 2,
        address: "10.0.0.2",
        rtt1Ms: 10,
        rtt2Ms: 11,
        rtt3Ms: 12,
        annotation: null,
        at: "2026-08-16T00:00:02.000Z",
      })[0],
    ]

    const result = applyHostnameEvent(completedRows, {
      event: "hostname",
      hop: 2,
      address: "10.0.0.2",
      hostname: "late.example.net",
    })

    expect(result[1].hostname).toBe("late.example.net")
  })
})

describe("compareTraceHops", () => {
  it("marks identical hops as same", () => {
    const a = [makeRow(1, "10.0.0.1", "a.example")]
    const b = [makeRow(1, "10.0.0.1", "a.example")]
    expect(compareTraceHops(a, b)).toEqual([
      { hop: 1, a: a[0], b: b[0], status: "same" },
    ])
  })

  it("marks hops with different RTTs as changed", () => {
    const a = [makeRow(1, "10.0.0.1", null)]
    const b = [{ ...a[0], rtt1Ms: 999 }]
    expect(compareTraceHops(a, b)[0].status).toBe("changed")
  })

  it("marks a hop missing from one side as a-only or b-only", () => {
    const a = [makeRow(1, "10.0.0.1", null)]
    const b = [makeRow(2, "10.0.0.2", null)]
    const result = compareTraceHops(a, b)
    expect(result).toHaveLength(2)
    expect(result.find((row) => row.hop === 1)?.status).toBe("a-only")
    expect(result.find((row) => row.hop === 2)?.status).toBe("b-only")
  })

  it("sorts the result by hop number even when inputs are unsorted", () => {
    const a = [makeRow(3, "10.0.0.3", null), makeRow(1, "10.0.0.1", null)]
    const b = [makeRow(2, "10.0.0.2", null), makeRow(1, "10.0.0.1", null)]
    expect(compareTraceHops(a, b).map((row) => row.hop)).toEqual([1, 2, 3])
  })
})
