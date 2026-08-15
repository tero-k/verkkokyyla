import { describe, expect, it } from "vitest"
import { downsample } from "./downsample"
import type { ProbeRow } from "./types"

type ProbePoint = {
  readonly x: number
  readonly rttMs: number | null
  readonly lost: boolean
}

function makeProbe(seq: number, rtt: number | null, lost: boolean): ProbePoint & ProbeRow {
  return { x: seq, seq, rttMs: rtt, lost, at: new Date(seq * 1000).toISOString() }
}

describe("downsample", () => {
  it("reduces a 2000-point series to at most twice the canvas width buckets", () => {
    const canvasWidth = 100
    const probes: ProbePoint[] = []
    for (let seq = 1; seq <= 2000; seq += 1) {
      const rtt = 10 + (seq % 50)
      probes.push(makeProbe(seq, rtt, false))
    }
    const result = downsample(probes, canvasWidth)
    expect(result.rtt.length).toBeLessThanOrEqual(canvasWidth * 2)
    expect(result.loss.length).toBeLessThanOrEqual(canvasWidth)
  })

  it("preserves the global maximum RTT", () => {
    const canvasWidth = 50
    const probes: ProbePoint[] = []
    for (let seq = 1; seq <= 500; seq += 1) {
      probes.push(makeProbe(seq, seq === 250 ? 999 : 10, false))
    }
    const result = downsample(probes, canvasWidth)
    const rttValues = result.rtt.map((point) => point.y).filter((y): y is number => y !== null)
    expect(rttValues).toContain(999)
    expect(result.globalMax).toBe(999)
  })

  it("preserves the loss count in the loss series", () => {
    const canvasWidth = 20
    const probes: ProbePoint[] = []
    for (let seq = 1; seq <= 100; seq += 1) {
      probes.push(makeProbe(seq, seq % 2 === 0 ? null : 5, seq % 2 === 0))
    }
    const result = downsample(probes, canvasWidth)
    const totalLoss = result.loss.reduce((sum, point) => sum + point.lossCount, 0)
    expect(totalLoss).toBe(50)
  })

  it("returns null RTT values at loss markers", () => {
    const canvasWidth = 10
    const probes: ProbePoint[] = []
    for (let seq = 1; seq <= 20; seq += 1) {
      probes.push(makeProbe(seq, seq === 10 ? null : 5, seq === 10))
    }
    const result = downsample(probes, canvasWidth)
    expect(result.rtt.some((point) => point.y === null)).toBe(true)
  })

  it("does not draw a continuous line across a loss gap", () => {
    const canvasWidth = 3
    const probes: ProbePoint[] = [
      makeProbe(1, 10, false),
      makeProbe(2, 10, false),
      makeProbe(3, null, true),
      makeProbe(4, 10, false),
      makeProbe(5, 10, false),
    ]
    const result = downsample(probes, canvasWidth)
    const rttValues = result.rtt.map((point) => point.y)
    let sawNonNull = false
    let sawNull = false
    let sawNonNullAfterNull = false
    for (const value of rttValues) {
      if (value !== null && !sawNull) sawNonNull = true
      if (value === null && sawNonNull) sawNull = true
      if (value !== null && sawNull) sawNonNullAfterNull = true
    }
    expect(sawNonNull && sawNull && sawNonNullAfterNull).toBe(true)
  })
})
