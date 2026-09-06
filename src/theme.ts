import { useEffect, useState } from "react"

const THEME_STORAGE_KEY = "verkkokyyla-theme"

type ThemeMode = "light" | "dark" | "system"
type ResolvedTheme = "light" | "dark"

function resolveTheme(mode: ThemeMode): ResolvedTheme {
  if (mode !== "system") return mode
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
}

function applyTheme(mode: ThemeMode): ResolvedTheme {
  const resolved = resolveTheme(mode)
  document.documentElement.dataset.theme = resolved
  document.documentElement.style.colorScheme = resolved
  return resolved
}

function readStoredTheme(): ThemeMode {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY)
    if (stored === "light" || stored === "dark" || stored === "system") return stored
  } catch {
    // localStorage may be unavailable in some environments.
  }
  return "system"
}

function storeTheme(mode: ThemeMode): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, mode)
  } catch {
    // ignore write failures
  }
}

export function initializeTheme(): ResolvedTheme {
  const mode = readStoredTheme()
  return applyTheme(mode)
}

export function useTheme(): {
  mode: ThemeMode
  resolved: ResolvedTheme
  setMode: (mode: ThemeMode) => void
} {
  const [mode, setModeState] = useState<ThemeMode>(readStoredTheme())
  const [resolved, setResolved] = useState<ResolvedTheme>(() => resolveTheme(mode))

  useEffect(() => {
    setResolved(applyTheme(mode))
  }, [mode])

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)")
    const listener = (event: MediaQueryListEvent) => {
      if (mode === "system") {
        setResolved(event.matches ? "dark" : "light")
        applyTheme("system")
      }
    }
    media.addEventListener("change", listener)
    return () => media.removeEventListener("change", listener)
  }, [mode])

  const setMode = (next: ThemeMode) => {
    storeTheme(next)
    setModeState(next)
  }

  return { mode, resolved, setMode }
}

export type { ThemeMode, ResolvedTheme }
