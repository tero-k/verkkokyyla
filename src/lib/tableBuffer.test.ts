import { describe, expect, it } from "vitest"
import { TableBuffer } from "./tableBuffer"
import type { ProbeRow } from "./types"

function makeRow(seq: number): ProbeRow {
  return { seq, rttMs: 10, lost: false, at: new Date(seq * 1000).toISOString() }
}

describe("TableBuffer", () => {
  it("drops the oldest rows once the cap is exceeded", () => {
    const buffer = new TableBuffer(500)
    for (let seq = 1; seq <= 600; seq += 1) {
      buffer.add(makeRow(seq))
    }
    const rows = buffer.all()
    expect(rows).toHaveLength(500)
    expect(rows[0].seq).toBe(101)
    expect(rows[rows.length - 1].seq).toBe(600)
  })
})
