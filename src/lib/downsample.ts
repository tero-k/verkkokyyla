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

function bucketJitter(rtts: readonly number[]): number | null {
  if (rtts.length < 2) return null
  let jitter = Math.abs(rtts[1] - rtts[0])
  for (let i = 2; i < rtts.length; i++) {
    const delta = Math.abs(rtts[i] - rtts[i - 1])
    jitter += (delta - jitter) / 16
  }
  return jitter
}

function aggregateBucket(bucket: readonly ProbePoint[]) {
  let count = 0
  let lossCount = 0
  let firstLossX: number | null = null
  let sumRtt = 0
  let minRtt: { readonly x: number; readonly rtt: number } | null = null
  let maxRtt: { readonly x: number; readonly rtt: number } | null = null
  const rtts: number[] = []

  for (const probe of bucket) {
    count += 1
    if (probe.lost || probe.rttMs === null) {
      lossCount += 1
      if (firstLossX === null) firstLossX = probe.x
      continue
    }
    const rtt = probe.rttMs
    sumRtt += rtt
    rtts.push(rtt)
    if (minRtt === null || rtt < minRtt.rtt) {
      minRtt = { x: probe.x, rtt }
    }
    if (maxRtt === null || rtt > maxRtt.rtt) {
      maxRtt = { x: probe.x, rtt }
    }
  }

  const avgRtt = rtts.length > 0 ? sumRtt / rtts.length : null
  const jitter = bucketJitter(rtts)

  return {
    count,
    lossCount,
    firstLossX,
    minRtt,
    maxRtt,
    avgRtt,
    jitter,
    lastX: bucket[bucket.length - 1]?.x ?? 0,
  }
}

export function downsample(probes: readonly ProbePoint[], canvasWidth: number): DownsampleResult {
  if (probes.length === 0) {
    return { rtt: [], loss: [], jitter: [], globalMin: null, globalMax: null }
  }

  const bucketCount = Math.max(1, Math.floor(canvasWidth))
  const bucketSize = probes.length / bucketCount

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

    const bucket = probes.slice(start, end)
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
