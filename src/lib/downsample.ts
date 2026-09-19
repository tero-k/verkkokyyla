type ProbePoint = {
  readonly x: number
  readonly rttMs: number | null
  readonly lost: boolean
}

export type RttPoint = { readonly x: number; readonly y: number | null }
export type LossPoint = { readonly x: number; readonly lossCount: number; readonly totalCount: number }
export type JitterPoint = { readonly x: number; readonly y: number | null }

export type DownsampleResult = {
  readonly rtt: readonly RttPoint[]
  readonly loss: readonly LossPoint[]
  readonly jitter: readonly JitterPoint[]
  readonly globalMin: number | null
  readonly globalMax: number | null
}

type EnrichedPoint = {
  readonly probe: ProbePoint
  readonly jitter: number | null
}

/// Running RFC 3550 jitter estimate per successful probe, mirroring
/// `computeSnapshot` in snapshot.ts: each probe gets the estimate computed
/// from consecutive successful RTTs, so the series starts at the second
/// successful probe and survives buckets that hold a single probe.
function attachJitter(probes: readonly ProbePoint[]): EnrichedPoint[] {
  let previousRtt: number | null = null
  let runningJitter: number | null = null
  return probes.map((probe) => {
    let jitter: number | null = null
    if (!probe.lost && probe.rttMs !== null) {
      if (previousRtt !== null) {
        const delta = Math.abs(probe.rttMs - previousRtt)
        runningJitter =
          runningJitter === null ? delta : runningJitter + (delta - runningJitter) / 16
        jitter = runningJitter
      }
      previousRtt = probe.rttMs
    }
    return { probe, jitter }
  })
}

function aggregateBucket(bucket: readonly EnrichedPoint[]) {
  let count = 0
  let lossCount = 0
  let firstLossX: number | null = null
  let sumRtt = 0
  let minRtt: { readonly x: number; readonly rtt: number } | null = null
  let maxRtt: { readonly x: number; readonly rtt: number } | null = null
  const rtts: number[] = []
  let sumJitter = 0
  let jitterCount = 0

  for (const { probe, jitter } of bucket) {
    count += 1
    if (probe.lost || probe.rttMs === null) {
      lossCount += 1
      if (firstLossX === null) firstLossX = probe.x
      continue
    }
    const rtt = probe.rttMs
    sumRtt += rtt
    rtts.push(rtt)
    if (jitter !== null) {
      sumJitter += jitter
      jitterCount += 1
    }
    if (minRtt === null || rtt < minRtt.rtt) {
      minRtt = { x: probe.x, rtt }
    }
    if (maxRtt === null || rtt > maxRtt.rtt) {
      maxRtt = { x: probe.x, rtt }
    }
  }

  const avgRtt = rtts.length > 0 ? sumRtt / rtts.length : null
  const jitter = jitterCount > 0 ? sumJitter / jitterCount : null

  return {
    count,
    lossCount,
    firstLossX,
    minRtt,
    maxRtt,
    avgRtt,
    jitter,
    lastX: bucket[bucket.length - 1]?.probe.x ?? 0,
  }
}

export function downsample(probes: readonly ProbePoint[], canvasWidth: number): DownsampleResult {
  if (probes.length === 0) {
    return { rtt: [], loss: [], jitter: [], globalMin: null, globalMax: null }
  }

  const bucketCount = Math.max(1, Math.floor(canvasWidth))
  const bucketSize = probes.length / bucketCount
  const enriched = attachJitter(probes)

  let globalMin: number | null = null
  let globalMax: number | null = null
  let minPoint: { readonly x: number; readonly rtt: number } | null = null
  let maxPoint: { readonly x: number; readonly rtt: number } | null = null

  for (const probe of probes) {
    if (probe.lost || probe.rttMs === null) continue
    const rtt = probe.rttMs
    if (globalMin === null || rtt < globalMin) {
      globalMin = rtt
      minPoint = { x: probe.x, rtt }
    }
    if (globalMax === null || rtt > globalMax) {
      globalMax = rtt
      maxPoint = { x: probe.x, rtt }
    }
  }

  const rtt: RttPoint[] = []
  const loss: LossPoint[] = []
  const jitter: JitterPoint[] = []

  for (let bucketIndex = 0; bucketIndex < bucketCount; bucketIndex++) {
    const start = Math.floor(bucketIndex * bucketSize)
    const end = Math.min(probes.length, Math.floor((bucketIndex + 1) * bucketSize))
    if (start >= end) continue

    const bucket = enriched.slice(start, end)
    const stats = aggregateBucket(bucket)

    const representativeX = stats.maxRtt?.x ?? stats.firstLossX ?? stats.lastX
    loss.push({ x: representativeX, lossCount: stats.lossCount, totalCount: stats.count })
    jitter.push({ x: representativeX, y: stats.jitter })

    if (stats.minRtt === null && stats.maxRtt === null) {
      rtt.push({ x: stats.lastX, y: null })
      continue
    }

    const minInBucket = stats.minRtt
    const maxInBucket = stats.maxRtt
    const hasLoss = stats.lossCount > 0
    const hasGlobalMin =
      minPoint !== null &&
      minInBucket !== null &&
      minInBucket.x === minPoint.x &&
      minInBucket.rtt === minPoint.rtt
    const hasGlobalMax =
      maxPoint !== null &&
      maxInBucket !== null &&
      maxInBucket.x === maxPoint.x &&
      maxInBucket.rtt === maxPoint.rtt

    if (!hasLoss) {
      if (
        minInBucket !== null &&
        maxInBucket !== null &&
        minInBucket.x === maxInBucket.x &&
        minInBucket.rtt === maxInBucket.rtt
      ) {
        rtt.push({ x: minInBucket.x, y: minInBucket.rtt })
      } else {
        if (minInBucket !== null) rtt.push({ x: minInBucket.x, y: minInBucket.rtt })
        if (maxInBucket !== null) rtt.push({ x: maxInBucket.x, y: maxInBucket.rtt })
      }
      continue
    }

    const lossX = stats.firstLossX ?? stats.lastX

    if (hasGlobalMin && hasGlobalMax && minInBucket !== null && maxInBucket !== null) {
      rtt.push({ x: minInBucket.x, y: minInBucket.rtt })
      rtt.push({ x: maxInBucket.x, y: maxInBucket.rtt })
    } else if (hasGlobalMin && minInBucket !== null) {
      rtt.push({ x: minInBucket.x, y: minInBucket.rtt })
      rtt.push({ x: lossX, y: null })
    } else if (maxInBucket !== null) {
      rtt.push({ x: maxInBucket.x, y: maxInBucket.rtt })
      rtt.push({ x: lossX, y: null })
    } else {
      rtt.push({ x: lossX, y: null })
    }
  }

  return { rtt, loss, jitter, globalMin, globalMax }
}
