import { describe, expect, it } from "vitest"
import {
  applyFiltersAndSort,
  compareResources,
  splitUrlForDisplay,
  type ResourceFilters,
  type SortKey,
} from "./pageResources"
import { PAGE_RESOURCE_TYPES, type PageResourceDto, type PageResourceType } from "./types"

function makeResource(
  url: string,
  overrides: Partial<PageResourceDto> = {},
): PageResourceDto {
  return {
    url,
    resourceType: "other",
    statusCode: 200,
    contentLength: 100,
    bytesReceived: 100,
    startOffsetMs: 0,
    durationMs: 100,
    timeToFirstByteMs: 10,
    averageMbps: 1,
    error: null,
    ...overrides,
  }
}

const ALL_TYPES: ReadonlySet<PageResourceType> = new Set(PAGE_RESOURCE_TYPES)

function makeFilters(overrides: Partial<ResourceFilters> = {}): ResourceFilters {
  return {
    types: ALL_TYPES,
    status: "all",
    slowestOnly: false,
    ...overrides,
  }
}

describe("compareResources", () => {
  it.each([
    ["type", makeResource("b", { resourceType: "script" }), makeResource("a", { resourceType: "image" })],
    ["url", makeResource("https://b.test"), makeResource("https://a.test")],
    ["status", makeResource("b", { statusCode: 404 }), makeResource("a", { statusCode: 200 })],
    ["size", makeResource("b", { bytesReceived: 200 }), makeResource("a", { bytesReceived: 100 })],
    ["start", makeResource("b", { startOffsetMs: 20 }), makeResource("a", { startOffsetMs: 10 })],
    ["duration", makeResource("b", { durationMs: 20 }), makeResource("a", { durationMs: 10 })],
    ["speed", makeResource("b", { averageMbps: 2 }), makeResource("a", { averageMbps: 1 })],
  ] satisfies readonly [SortKey, PageResourceDto, PageResourceDto][])(
    "compares %s values in ascending order",
    (key, later, earlier) => {
      expect(compareResources(later, earlier, key)).toBeGreaterThan(0)
      expect(compareResources(earlier, later, key)).toBeLessThan(0)
    },
  )

  it("places a missing status after a present status", () => {
    const missing = makeResource("missing", { statusCode: null })
    const present = makeResource("present", { statusCode: 200 })

    expect(compareResources(missing, present, "status")).toBeGreaterThan(0)
    expect(compareResources(present, missing, "status")).toBeLessThan(0)
  })

  it("places errors before successful resources and compares their messages", () => {
    const alpha = makeResource("alpha", { error: "alpha" })
    const beta = makeResource("beta", { error: "beta" })
    const successful = makeResource("successful")

    expect(compareResources(alpha, successful, "error")).toBeLessThan(0)
    expect(compareResources(alpha, beta, "error")).toBeLessThan(0)
  })
})

describe("applyFiltersAndSort", () => {
  it("filters by all selected resource types before sorting", () => {
    const resources = [
      makeResource("script-b", { resourceType: "script", startOffsetMs: 20 }),
      makeResource("image", { resourceType: "image", startOffsetMs: 10 }),
      makeResource("script-a", { resourceType: "script", startOffsetMs: 5 }),
    ]

    const result = applyFiltersAndSort(
      resources,
      makeFilters({ types: new Set(["script"]) }),
      "start",
      "asc",
    )

    expect(result.map((resource) => resource.url)).toEqual(["script-a", "script-b"])
  })

  it.each([
    ["2xx", 204],
    ["3xx", 302],
    ["4xx", 404],
    ["5xx", 503],
  ] as const)("matches the %s status bucket", (status, statusCode) => {
    const resources = [
      makeResource("matching", { statusCode }),
      makeResource("other", { statusCode: 100 }),
      makeResource("missing", { statusCode: null }),
    ]

    const result = applyFiltersAndSort(
      resources,
      makeFilters({ status }),
      "start",
      "asc",
    )

    expect(result.map((resource) => resource.url)).toEqual(["matching"])
  })

  it("matches failed resources by error even when status is missing", () => {
    const resources = [
      makeResource("failed-with-status", { statusCode: 500, error: "server error" }),
      makeResource("failed-without-status", { statusCode: null, error: "network error" }),
      makeResource("status-only", { statusCode: 500 }),
    ]

    const result = applyFiltersAndSort(
      resources,
      makeFilters({ status: "failed" }),
      "url",
      "asc",
    )

    expect(result.map((resource) => resource.url)).toEqual([
      "failed-with-status",
      "failed-without-status",
    ])
  })

  it("keeps only the five slowest resource URLs", () => {
    const resources = Array.from({ length: 7 }, (_, index) =>
      makeResource(`resource-${index}`, { durationMs: index * 10 }),
    )

    const result = applyFiltersAndSort(
      resources,
      makeFilters({ slowestOnly: true }),
      "duration",
      "desc",
    )

    expect(result.map((resource) => resource.url)).toEqual([
      "resource-6",
      "resource-5",
      "resource-4",
      "resource-3",
      "resource-2",
    ])
  })

  it("treats resources sharing a URL as distinct when keeping the slowest", () => {
    const resources = [
      makeResource("https://a.test/", { resourceType: "document", durationMs: 300 }),
      makeResource("https://a.test/", { resourceType: "other", durationMs: 200 }),
      ...Array.from({ length: 5 }, (_, index) =>
        makeResource(`https://a.test/filler-${index}.css`, { durationMs: 10 - index }),
      ),
    ]

    const result = applyFiltersAndSort(
      resources,
      makeFilters({ slowestOnly: true }),
      "duration",
      "desc",
    )

    expect(result).toHaveLength(5)
    expect(result[0].resourceType).toBe("document")
    expect(result[1].resourceType).toBe("other")
    expect(result[1].url).toBe("https://a.test/")
  })

  it("sorts descending while keeping missing statuses last", () => {
    const resources = [
      makeResource("missing", { statusCode: null }),
      makeResource("success", { statusCode: 200 }),
      makeResource("error", { statusCode: 500 }),
    ]

    const result = applyFiltersAndSort(resources, makeFilters(), "status", "desc")

    expect(result.map((resource) => resource.url)).toEqual(["error", "success", "missing"])
  })

  it("breaks sort ties by URL", () => {
    const resources = [
      makeResource("https://c.test/z", { durationMs: 100 }),
      makeResource("https://a.test/z", { durationMs: 100 }),
      makeResource("https://b.test/z", { durationMs: 100 }),
    ]

    const result = applyFiltersAndSort(resources, makeFilters(), "duration", "asc")

    expect(result.map((resource) => resource.url)).toEqual([
      "https://a.test/z",
      "https://b.test/z",
      "https://c.test/z",
    ])
  })

  it("preserves input order when both sort value and URL compare equally", () => {
    const resources = [
      makeResource("https://a.test/z", { durationMs: 100 }),
      makeResource("https://a.test/z", { durationMs: 100 }),
    ]

    const result = applyFiltersAndSort(resources, makeFilters(), "duration", "asc")

    expect(result).toHaveLength(2)
    expect(result[0]).toBe(resources[0])
    expect(result[1]).toBe(resources[1])
  })

  it("returns a new array without mutating the input", () => {
    const resources = [
      makeResource("later", { startOffsetMs: 20 }),
      makeResource("earlier", { startOffsetMs: 10 }),
    ]

    const result = applyFiltersAndSort(resources, makeFilters(), "start", "asc")

    expect(result).not.toBe(resources)
    expect(resources.map((resource) => resource.url)).toEqual(["later", "earlier"])
  })
})

describe("splitUrlForDisplay", () => {
  it("splits a deep path into head and tail at the last slash", () => {
    expect(splitUrlForDisplay("https://www.is.fi/_next/static/chunks/9521.abc.js")).toEqual({
      head: "https://www.is.fi/_next/static/chunks/",
      tail: "9521.abc.js",
    })
  })

  it("keeps query string and fragment in the tail", () => {
    expect(splitUrlForDisplay("https://a.test/lib/app.js?v=2#top")).toEqual({
      head: "https://a.test/lib/",
      tail: "app.js?v=2#top",
    })
  })

  it("returns an origin-only URL whole", () => {
    expect(splitUrlForDisplay("https://www.is.fi/")).toEqual({
      head: "https://www.is.fi/",
      tail: "",
    })
    expect(splitUrlForDisplay("https://assets.adobedtm.com")).toEqual({
      head: "https://assets.adobedtm.com",
      tail: "",
    })
  })

  it("never splits inside the scheme separator", () => {
    const { head, tail } = splitUrlForDisplay("https://a.test")
    expect(head).toBe("https://a.test")
    expect(tail).toBe("")
  })
})
