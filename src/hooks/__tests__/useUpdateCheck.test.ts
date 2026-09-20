import { describe, expect, it } from "vitest"
import {
  readDismissedVersion,
  readUpdateCheckEnabled,
  shouldShowUpdate,
  UPDATE_CHECK_ENABLED_KEY,
  UPDATE_DISMISSED_KEY,
  writeDismissedVersion,
  writeUpdateCheckEnabled,
} from "../useUpdateCheck"

function makeStorage(entries: Record<string, string> = {}): Storage {
  const data = new Map<string, string>(Object.entries(entries))
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
    clear: () => data.clear(),
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    get length() {
      return data.size
    },
  }
}

const UPDATE = { version: "0.2.0", url: "https://example.test", current: "0.1.3" }

describe("readUpdateCheckEnabled", () => {
  it("defaults to enabled when storage is empty", () => {
    expect(readUpdateCheckEnabled(makeStorage())).toBe(true)
  })

  it("reads an explicit opt-out", () => {
    const storage = makeStorage({ [UPDATE_CHECK_ENABLED_KEY]: "false" })
    expect(readUpdateCheckEnabled(storage)).toBe(false)
  })

  it("reads an explicit opt-in", () => {
    const storage = makeStorage({ [UPDATE_CHECK_ENABLED_KEY]: "true" })
    expect(readUpdateCheckEnabled(storage)).toBe(true)
  })
})

describe("writeUpdateCheckEnabled", () => {
  it("round-trips the opt-out flag", () => {
    const storage = makeStorage()
    writeUpdateCheckEnabled(storage, false)
    expect(storage.getItem(UPDATE_CHECK_ENABLED_KEY)).toBe("false")
    expect(readUpdateCheckEnabled(storage)).toBe(false)

    writeUpdateCheckEnabled(storage, true)
    expect(readUpdateCheckEnabled(storage)).toBe(true)
  })
})

describe("dismissed version", () => {
  it("round-trips the dismissed version", () => {
    const storage = makeStorage()
    expect(readDismissedVersion(storage)).toBeNull()

    writeDismissedVersion(storage, "0.2.0")
    expect(storage.getItem(UPDATE_DISMISSED_KEY)).toBe("0.2.0")
    expect(readDismissedVersion(storage)).toBe("0.2.0")
  })
})

describe("shouldShowUpdate", () => {
  it("shows an undismissed update", () => {
    expect(shouldShowUpdate(UPDATE, null)).toBe(true)
    expect(shouldShowUpdate(UPDATE, "0.1.9")).toBe(true)
  })

  it("hides a dismissed update", () => {
    expect(shouldShowUpdate(UPDATE, "0.2.0")).toBe(false)
  })

  it("hides when there is no update", () => {
    expect(shouldShowUpdate(null, null)).toBe(false)
    expect(shouldShowUpdate(null, "0.2.0")).toBe(false)
  })
})
