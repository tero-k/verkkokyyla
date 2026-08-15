import { useCallback, useState } from "react"
import { runDownloadSpeedTest } from "../lib/ipc"
import type { DownloadProgressEvent, DownloadSpeedResultDto } from "../lib/types"

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
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [progress, setProgress] = useState<DownloadProgressEvent | null>(null)
  const [result, setResult] = useState<DownloadSpeedResultDto | null>(null)

  const reset = useCallback(() => {
    setError("")
    setProgress(null)
    setResult(null)
  }, [])

  const start = useCallback(async () => {
    reset()
    setIsRunning(true)
    try {
      const finalResult = await runDownloadSpeedTest(url, (event) => {
        setProgress(event)
      })
      setResult(finalResult)
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setIsRunning(false)
    }
  }, [reset, url])

  return {
    url,
    setUrl,
    isRunning,
    isValid: isValidUrl(url),
    error,
    progress,
    result,
    start,
    reset,
  }
}
