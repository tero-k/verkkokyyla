import { useCallback, useEffect, useState } from "react"
import { checkForUpdate } from "../lib/ipc"
import type { UpdateInfo } from "../lib/types"

export const UPDATE_CHECK_ENABLED_KEY = "verkkokyyla-update-check-enabled-v1"
export const UPDATE_DISMISSED_KEY = "verkkokyyla-update-dismissed-v1"

type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">

/** Delay before the background check so it stays off the startup path. */
export const UPDATE_CHECK_DELAY_MS = 2000

/** The check is on by default; only an explicit "false" opts out. */
export function readUpdateCheckEnabled(
  storage: Pick<Storage, "getItem">,
): boolean {
  try {
    return storage.getItem(UPDATE_CHECK_ENABLED_KEY) !== "false"
  } catch {
    // localStorage may be unavailable; keep the convenience feature on.
    return true
  }
}

export function writeUpdateCheckEnabled(
  storage: StorageLike,
  enabled: boolean,
): void {
  try {
    storage.setItem(UPDATE_CHECK_ENABLED_KEY, enabled ? "true" : "false")
  } catch {
    // Storage may be disabled or full; keep the in-memory state.
  }
}

export function readDismissedVersion(
  storage: Pick<Storage, "getItem">,
): string | null {
  try {
    return storage.getItem(UPDATE_DISMISSED_KEY)
  } catch {
    return null
  }
}

export function writeDismissedVersion(
  storage: StorageLike,
  version: string,
): void {
  try {
    storage.setItem(UPDATE_DISMISSED_KEY, version)
  } catch {
    // Storage may be disabled or full; dismissal just won't persist.
  }
}

/** A notice shows only while its version is undismissed. */
export function shouldShowUpdate(
  update: UpdateInfo | null,
  dismissedVersion: string | null,
): update is UpdateInfo {
  return update !== null && update.version !== dismissedVersion
}

/**
 * Check GitHub Releases once per app launch (when enabled) and remember the
 * opt-out and per-version dismissal flags, mirroring usePortScanConsent's
 * localStorage convention.
 *
 * The effect has no "already started" guard on purpose: React StrictMode
 * mounts, cleans up, and remounts effects in dev, so the guard would block
 * the remounted timer (which is the one that actually runs) and the check
 * would never fire. Cleanup-owned cancellation gives exactly one check per
 * mount in dev and in production alike.
 */
export function useUpdateCheck(storage: StorageLike = localStorage) {
  const [enabled, setEnabledState] = useState(() =>
    readUpdateCheckEnabled(storage),
  )
  const [update, setUpdate] = useState<UpdateInfo | null>(null)
  const [dismissedVersion, setDismissedVersion] = useState(() =>
    readDismissedVersion(storage),
  )

  useEffect(() => {
    if (!enabled) return
    let cancelled = false
    const timer = window.setTimeout(() => {
      if (cancelled) return
      checkForUpdate()
        .then((info) => {
          if (!cancelled) setUpdate(info)
        })
        .catch(() => {
          // A failed check stays silent: no error UI for a background
          // convenience feature.
        })
    }, UPDATE_CHECK_DELAY_MS)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [enabled])

  const dismiss = useCallback(() => {
    if (update === null) return
    writeDismissedVersion(storage, update.version)
    setDismissedVersion(update.version)
  }, [storage, update])

  const setEnabled = useCallback(
    (value: boolean) => {
      writeUpdateCheckEnabled(storage, value)
      setEnabledState(value)
    },
    [storage],
  )

  return {
    enabled,
    setEnabled,
    visibleUpdate: shouldShowUpdate(update, dismissedVersion) ? update : null,
    dismiss,
  }
}
