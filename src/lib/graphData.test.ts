import { describe, expect, it } from "vitest"
import { buildGraphData } from "./graphData"
import type { ProbeRow } from "./types"

function probe(seq: number, at: string, rttMs: number | null): ProbeRow {
  return { seq, at, rttMs, lost: rttMs === null }
}

describe("buildGraphData", () => {
  it("converts RFC3339 probe timestamps to Unix seconds for uPlot", () => {
    const probes = [
      probe(1, "2026-08-30T16:00:00Z", 10),
      probe(2, "2026-08-30T16:00:01Z", 12),
    ]
    const data = buildGraphData(probes, 1000)
    expect(data.rtt[0]?.x).toBe(Date.UTC(2026, 7, 30, 16, 0, 0) / 1000)
    expect(data.rtt[1]?.x).toBe(Date.UTC(2026, 7, 30, 16, 0, 1) / 1000)
  })

  it("falls back to seq when the timestamp does not parse", () => {
    const data = buildGraphData([probe(7, "not-a-date", 5)], 1000)
    expect(data.rtt[0]?.x).toBe(7)
  })
})
