import type { PageResourceDto, PageResourceType } from "./types"

export type SortKey =
  | "type"
  | "url"
  | "status"
  | "size"
  | "start"
  | "duration"
  | "speed"
  | "error"

export type SortDir = "asc" | "desc"

export type StatusFilter = "all" | "2xx" | "3xx" | "4xx" | "5xx" | "failed"

export type ResourceFilters = {
  readonly types: ReadonlySet<PageResourceType>
  readonly status: StatusFilter
  readonly slowestOnly: boolean
}

function assertNever(value: never): never {
  throw new Error(`Unexpected sort key: ${String(value)}`)
}

export function compareResources(
  a: PageResourceDto,
  b: PageResourceDto,
  key: SortKey,
): number {
  switch (key) {
    case "type":
      return a.resourceType.localeCompare(b.resourceType)
    case "url":
      return a.url.localeCompare(b.url)
    case "status":
      if (a.statusCode === null) return b.statusCode === null ? 0 : 1
      if (b.statusCode === null) return -1
      return a.statusCode - b.statusCode
    case "size":
      return a.bytesReceived - b.bytesReceived
    case "start":
      return a.startOffsetMs - b.startOffsetMs
    case "duration":
      return a.durationMs - b.durationMs
    case "speed":
      return a.averageMbps - b.averageMbps
    case "error":
      if (a.error === null) return b.error === null ? 0 : 1
      if (b.error === null) return -1
      return a.error.localeCompare(b.error)
    default:
      return assertNever(key)
  }
}

function matchesStatus(resource: PageResourceDto, status: StatusFilter): boolean {
  switch (status) {
    case "all":
      return true
    case "failed":
      return resource.error !== null
    case "2xx":
      return resource.statusCode !== null && Math.floor(resource.statusCode / 100) === 2
    case "3xx":
      return resource.statusCode !== null && Math.floor(resource.statusCode / 100) === 3
    case "4xx":
      return resource.statusCode !== null && Math.floor(resource.statusCode / 100) === 4
    case "5xx":
      return resource.statusCode !== null && Math.floor(resource.statusCode / 100) === 5
    default:
      return assertNever(status)
  }
}

export function applyFiltersAndSort(
  resources: readonly PageResourceDto[],
  filters: ResourceFilters,
  sortKey: SortKey,
  sortDir: SortDir,
): PageResourceDto[] {
  // Identity-based: distinct resources may legitimately share a URL (the page
  // document and a self-referencing <link>), so key by object reference.
  const slowest = filters.slowestOnly
    ? new Set([...resources].sort((a, b) => b.durationMs - a.durationMs).slice(0, 5))
    : null

  return resources
    .filter(
      (resource) =>
        filters.types.has(resource.resourceType) &&
        matchesStatus(resource, filters.status) &&
        (slowest === null || slowest.has(resource)),
    )
    .map((resource, index) => ({ resource, index }))
    .sort((a, b) => {
      const comparison = compareResources(a.resource, b.resource, sortKey)
      if (comparison !== 0) {
        if (
          sortKey === "status" &&
          (a.resource.statusCode === null || b.resource.statusCode === null)
        ) {
          return comparison
        }
        return sortDir === "asc" ? comparison : -comparison
      }
      // Break ties by URL so every sort produces a deterministic order.
      const urlComparison = a.resource.url.localeCompare(b.resource.url)
      if (urlComparison !== 0) return urlComparison
      return a.index - b.index
    })
    .map(({ resource }) => resource)
}

/**
 * Split a URL into a head (origin + path up to the last slash) and a tail
 * (final segment plus any query/fragment) so the table cell can truncate the
 * middle: the head ellipsizes first while the distinguishing tail stays
 * visible.
 */
export function splitUrlForDisplay(url: string): { head: string; tail: string } {
  const lastSlash = url.lastIndexOf("/")
  // "https://".length === 8 — never split inside the scheme separator, and
  // keep bare hosts ("https://example.com") or origin-only URLs whole.
  if (lastSlash <= 8) return { head: url, tail: "" }
  const head = url.slice(0, lastSlash + 1)
  const tail = url.slice(lastSlash + 1)
  if (tail.length === 0) return { head: url, tail: "" }
  return { head, tail }
}
