import { downsample } from "./downsample"
import type { ProbeRow } from "./types"

type GraphPoint = {
  readonly x: number
  readonly rttMs: number | null
  readonly lost: boolean
}

function probeX(probe: ProbeRow): number {
  const time = new Date(probe.at).getTime()
  // uPlot's time scale expects Unix seconds, not milliseconds.
  return Number.isNaN(time) ? probe.seq : time / 1000
}

export function buildGraphData(probes: readonly ProbeRow[], canvasWidth: number) {
  const points: GraphPoint[] = probes.map((probe) => ({
    x: probeX(probe),
    rttMs: probe.rttMs,
    lost: probe.lost,
  }))

  return downsample(points, canvasWidth)
}
