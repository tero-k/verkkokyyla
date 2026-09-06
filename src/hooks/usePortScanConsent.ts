import { useCallback, useState } from "react"

const STORAGE_KEY = "verkkokyyla-port-scan-consent-v1"

function readConsent(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true"
  } catch {
    // localStorage may be unavailable (e.g. private mode); treat as not consented.
    return false
  }
}

function writeConsent(value: boolean): void {
  try {
    if (value) {
      localStorage.setItem(STORAGE_KEY, "true")
    } else {
      localStorage.removeItem(STORAGE_KEY)
    }
  } catch {
    // localStorage may be unavailable; ignore persistence failure.
  }
}

export function usePortScanConsent() {
  const [hasConsented, setHasConsented] = useState(readConsent)

  const recordConsent = useCallback(() => {
    writeConsent(true)
    setHasConsented(true)
  }, [])

  const clearConsent = useCallback(() => {
    writeConsent(false)
    setHasConsented(false)
  }, [])

  return { hasConsented, recordConsent, clearConsent }
}
