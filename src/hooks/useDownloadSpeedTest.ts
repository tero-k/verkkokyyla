import { useCallback, useState } from "react"
import { runDownloadSpeedTest, runPageSpeedTest } from "../lib/ipc"
import type {
  DownloadProgressEvent,
  DownloadSpeedResultDto,
  PageProgressEvent,
  PageSpeedResultDto,
} from "../lib/types"
import { useHttpSettings } from "./useHttpSettings"

export type SpeedMode = "single" | "page"

type SingleProgress = { readonly kind: "single"; readonly event: DownloadProgressEvent }
type PageProgress = { readonly kind: "page"; readonly event: PageProgressEvent }
export type SpeedProgress = SingleProgress | PageProgress

type SingleResult = { readonly kind: "single"; readonly data: DownloadSpeedResultDto }
type PageResult = { readonly kind: "page"; readonly data: PageSpeedResultDto }
export type SpeedResult = SingleResult | PageResult

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (
    typeof err === "object" &&
    err !== null &&
    "message" in err &&
    typeof (err as { message: unknown }).message === "string"
  ) {
    return (err as { message: string }).message
  }
  if (typeof err === "string") return err
  return String(err)
}

function isValidUrl(value: string): boolean {
  return /^https?:\/\//i.test(value.trim())
}

export function useDownloadSpeedTest() {
  const [url, setUrl] = useState("")
  const [mode, setMode] = useState<SpeedMode>("single")
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [progress, setProgress] = useState<SpeedProgress | null>(null)
  const [result, setResult] = useState<SpeedResult | null>(null)
  const { settings: httpSettings, update: updateHttpSettings, reset: resetHttpSettings } = useHttpSettings()

  const reset = useCallback(() => {
    setError("")
    setProgress(null)
    setResult(null)
  }, [])

  const start = useCallback(async () => {
    reset()
    setIsRunning(true)
    try {
      if (mode === "single") {
        const finalResult = await runDownloadSpeedTest(url, httpSettings, (event) => {
          setProgress({ kind: "single", event })
        })
        setResult({ kind: "single", data: finalResult })
      } else {
        const finalResult = await runPageSpeedTest(url, httpSettings, (event) => {
          setProgress({ kind: "page", event })
        })
        setResult({ kind: "page", data: finalResult })
      }
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setIsRunning(false)
    }
  }, [reset, url, mode, httpSettings])

  return {
    url,
    setUrl,
    mode,
    setMode,
    isRunning,
    isValid: isValidUrl(url),
    error,
    progress,
    result,
    start,
    reset,
    httpSettings,
    updateHttpSettings,
    resetHttpSettings,
  }
}
