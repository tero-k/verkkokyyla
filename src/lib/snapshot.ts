import type { ProbeRow, SnapshotDto } from "./types"

export function computeSnapshot(probes: readonly ProbeRow[]): SnapshotDto {
  const count = probes.length
  let lossCount = 0
  let minMs: number | null = null
  let maxMs: number | null = null
  let mean = 0
  let m2 = 0
  let n = 0
  let jitterMs: number | null = null
  let previousRtt: number | null = null

  for (const probe of probes) {
    if (probe.lost || probe.rttMs === null) {
      lossCount += 1
      continue
    }
    const rtt = probe.rttMs
    n += 1
    const delta = rtt - mean
    mean += delta / n
    m2 += delta * (rtt - mean)

    if (minMs === null || rtt < minMs) minMs = rtt
    if (maxMs === null || rtt > maxMs) maxMs = rtt

    if (previousRtt !== null) {
      const deltaJitter = Math.abs(rtt - previousRtt)
      jitterMs = jitterMs === null ? deltaJitter : jitterMs + (deltaJitter - jitterMs) / 16
    }
    previousRtt = rtt
  }

  const avgMs = n > 0 ? mean : null
  const variance = n > 0 ? m2 / n : null
  const stddevMs = variance !== null && variance >= 0 ? Math.sqrt(variance) : null

  return {
    count,
    lossCount,
    lossFraction: count > 0 ? lossCount / count : 0,
    minMs,
    avgMs,
    maxMs,
    stddevMs,
    jitterMs,
  }
}
