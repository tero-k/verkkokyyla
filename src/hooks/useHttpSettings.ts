import { useCallback, useEffect, useState } from "react"
import {
  DEFAULT_HTTP_SETTINGS,
  HTTP_VERSIONS,
  IP_FAMILIES,
  type HttpSettings,
  type HttpVersion,
  type IpFamily,
} from "../lib/types"

export const HTTP_SETTINGS_STORAGE_KEY = "verkkokyyla-http-settings-v2"

function isHttpVersion(value: unknown): value is HttpVersion {
  return typeof value === "string" && HTTP_VERSIONS.includes(value as HttpVersion)
}

function isIpFamily(value: unknown): value is IpFamily {
  return typeof value === "string" && IP_FAMILIES.includes(value as IpFamily)
}

function isNonNegativeNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
}

export function loadStoredSettings(
  storage: Pick<Storage, "getItem">,
): HttpSettings {
  const raw = storage.getItem(HTTP_SETTINGS_STORAGE_KEY)
  if (raw === null) return DEFAULT_HTTP_SETTINGS
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>
    return {
      version: isHttpVersion(parsed.version)
        ? parsed.version
        : DEFAULT_HTTP_SETTINGS.version,
      connectTimeoutSec: isNonNegativeNumber(parsed.connectTimeoutSec)
        ? parsed.connectTimeoutSec
        : DEFAULT_HTTP_SETTINGS.connectTimeoutSec,
      requestTimeoutSec: isNonNegativeNumber(parsed.requestTimeoutSec)
        ? parsed.requestTimeoutSec
        : DEFAULT_HTTP_SETTINGS.requestTimeoutSec,
      readTimeoutSec: isNonNegativeNumber(parsed.readTimeoutSec)
        ? parsed.readTimeoutSec
        : DEFAULT_HTTP_SETTINGS.readTimeoutSec,
      followRedirects:
        typeof parsed.followRedirects === "boolean"
          ? parsed.followRedirects
          : DEFAULT_HTTP_SETTINGS.followRedirects,
      maxRedirects: isNonNegativeNumber(parsed.maxRedirects)
        ? parsed.maxRedirects
        : DEFAULT_HTTP_SETTINGS.maxRedirects,
      compression:
        typeof parsed.compression === "boolean"
          ? parsed.compression
          : DEFAULT_HTTP_SETTINGS.compression,
      ipFamily: isIpFamily(parsed.ipFamily)
        ? parsed.ipFamily
        : DEFAULT_HTTP_SETTINGS.ipFamily,
      userAgent:
        typeof parsed.userAgent === "string"
          ? parsed.userAgent
          : DEFAULT_HTTP_SETTINGS.userAgent,
    }
  } catch {
    return DEFAULT_HTTP_SETTINGS
  }
}

export function saveStoredSettings(
  storage: Pick<Storage, "setItem">,
  settings: HttpSettings,
): void {
  storage.setItem(HTTP_SETTINGS_STORAGE_KEY, JSON.stringify(settings))
}

export function clearStoredSettings(
  storage: Pick<Storage, "removeItem">,
): void {
  storage.removeItem(HTTP_SETTINGS_STORAGE_KEY)
}

export function useHttpSettings() {
  const [settings, setSettings] = useState<HttpSettings>(() =>
    loadStoredSettings(localStorage),
  )

  useEffect(() => {
    try {
      saveStoredSettings(localStorage, settings)
    } catch {
      // Storage may be disabled or full; silently fail and keep in-memory state.
    }
  }, [settings])

  const update = useCallback((patch: Partial<HttpSettings>) => {
    setSettings((current) => ({ ...current, ...patch }))
  }, [])

  const reset = useCallback(() => {
    setSettings(DEFAULT_HTTP_SETTINGS)
    try {
      clearStoredSettings(localStorage)
    } catch {
      // Ignore storage errors on reset.
    }
  }, [])

  return {
    settings,
    update,
    reset,
  }
}
