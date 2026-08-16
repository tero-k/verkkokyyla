import { describe, expect, it } from "vitest"
import { DEFAULT_HTTP_SETTINGS } from "../../lib/types"
import {
  clearStoredSettings,
  HTTP_SETTINGS_STORAGE_KEY,
  loadStoredSettings,
  saveStoredSettings,
} from "../useHttpSettings"

function makeStorage(entries: Record<string, string> = {}): Storage {
  const data = new Map<string, string>(Object.entries(entries))
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
    clear: () => data.clear(),
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    get length() {
      return data.size
    },
  }
}

describe("loadStoredSettings", () => {
  it("returns defaults when storage is empty", () => {
    const storage = makeStorage()
    expect(loadStoredSettings(storage)).toEqual(DEFAULT_HTTP_SETTINGS)
  })

  it("loads persisted settings from storage", () => {
    const storage = makeStorage({
      [HTTP_SETTINGS_STORAGE_KEY]: JSON.stringify({
        ...DEFAULT_HTTP_SETTINGS,
        userAgent: "custom-agent/1.0",
        compression: false,
      }),
    })
    const settings = loadStoredSettings(storage)
    expect(settings.userAgent).toBe("custom-agent/1.0")
    expect(settings.compression).toBe(false)
  })

  it("falls back to defaults for invalid stored data", () => {
    const storage = makeStorage({
      [HTTP_SETTINGS_STORAGE_KEY]: "not-json",
    })
    expect(loadStoredSettings(storage)).toEqual(DEFAULT_HTTP_SETTINGS)
  })

  it("falls back to defaults for missing fields", () => {
    const storage = makeStorage({
      [HTTP_SETTINGS_STORAGE_KEY]: JSON.stringify({ userAgent: "x" }),
    })
    const settings = loadStoredSettings(storage)
    expect(settings.version).toBe(DEFAULT_HTTP_SETTINGS.version)
    expect(settings.userAgent).toBe("x")
  })
})

describe("saveStoredSettings", () => {
  it("writes settings to storage", () => {
    const storage = makeStorage()
    saveStoredSettings(storage, { ...DEFAULT_HTTP_SETTINGS, connectTimeoutSec: 42 })
    const raw = storage.getItem(HTTP_SETTINGS_STORAGE_KEY)
    expect(raw).toBeTruthy()
    expect(JSON.parse(raw ?? "{}").connectTimeoutSec).toBe(42)
  })
})

describe("clearStoredSettings", () => {
  it("removes settings from storage", () => {
    const storage = makeStorage({
      [HTTP_SETTINGS_STORAGE_KEY]: JSON.stringify(DEFAULT_HTTP_SETTINGS),
    })
    clearStoredSettings(storage)
    expect(storage.getItem(HTTP_SETTINGS_STORAGE_KEY)).toBeNull()
  })
})
