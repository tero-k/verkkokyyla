import { useCallback, useEffect, useState } from "react"
import {
  deleteDownloadSpeedSession,
  listDownloadSpeedSessions,
  loadDownloadSpeedSession,
  runDownloadSpeedTest,
  runPageSpeedTest,
  runWebBenchmark,
  saveDownloadSpeedSession,
} from "../lib/ipc"
import type {
  ConnectionMode,
  DownloadProgressEvent,
  DownloadSpeedResultDto,
  DownloadSpeedSessionSummaryDto,
  HttpVersion,
  LoadedDownloadSpeedSessionDto,
  PageProgressEvent,
  PageSpeedResultDto,
  WebBenchmarkConfig,
  WebBenchmarkResult,
} from "../lib/types"
import { useHttpSettings } from "./useHttpSettings"

export type SpeedMode = "single" | "page" | "benchmark"

const DEFAULT_BENCHMARK_CONFIG = {
  protocols: ["auto"] as const,
  runs: 10,
  connectionMode: "cold" as ConnectionMode,
  concurrency: null as number | null,
  probe: false,
} as const

type BenchmarkConfigState = {
  protocols: readonly HttpVersion[]
  runs: number
  connectionMode: ConnectionMode
  concurrency: number | null
  probe: boolean
}

type SingleProgress = { readonly kind: "single"; readonly event: DownloadProgressEvent }
type PageProgress = { readonly kind: "page"; readonly event: PageProgressEvent }
export type SpeedProgress = SingleProgress | PageProgress

type SingleResult = { readonly kind: "single"; readonly data: DownloadSpeedResultDto }
type PageResult = { readonly kind: "page"; readonly data: PageSpeedResultDto }
type BenchmarkResult = { readonly kind: "benchmark"; readonly data: WebBenchmarkResult }
export type SpeedResult = SingleResult | PageResult | BenchmarkResult

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
  const [benchmarkConfig, setBenchmarkConfig] = useState<BenchmarkConfigState>({
    protocols: ["auto"] as const,
    runs: DEFAULT_BENCHMARK_CONFIG.runs,
    connectionMode: DEFAULT_BENCHMARK_CONFIG.connectionMode,
    concurrency: DEFAULT_BENCHMARK_CONFIG.concurrency,
    probe: DEFAULT_BENCHMARK_CONFIG.probe,
  })
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState("")
  const [progress, setProgress] = useState<SpeedProgress | null>(null)
  const [result, setResult] = useState<SpeedResult | null>(null)
  const [sessions, setSessions] = useState<readonly DownloadSpeedSessionSummaryDto[]>([])
  const [sessionsLoading, setSessionsLoading] = useState(false)
  const { settings: httpSettings, update: updateHttpSettings, reset: resetHttpSettings } = useHttpSettings()

  const refreshSessions = useCallback(async () => {
    setSessionsLoading(true)
    try {
      const list = await listDownloadSpeedSessions()
      setSessions(list)
    } catch (err) {
      console.error("Failed to list download speed sessions", err)
    } finally {
      setSessionsLoading(false)
    }
  }, [])

  useEffect(() => {
    void refreshSessions()
  }, [refreshSessions])

  const reset = useCallback(() => {
    setError("")
    setProgress(null)
    setResult(null)
  }, [])

  const resetBenchmarkConfig = useCallback(() => {
    setBenchmarkConfig({
      protocols: ["auto"] as const,
      runs: DEFAULT_BENCHMARK_CONFIG.runs,
      connectionMode: DEFAULT_BENCHMARK_CONFIG.connectionMode,
      concurrency: DEFAULT_BENCHMARK_CONFIG.concurrency,
      probe: DEFAULT_BENCHMARK_CONFIG.probe,
    })
  }, [])

  const loadSession = useCallback(async (id: number) => {
    const loaded: LoadedDownloadSpeedSessionDto = await loadDownloadSpeedSession(id)
    const parsed = JSON.parse(loaded.resultJson)
    const sessionMode = loaded.session.mode
    const speedResult: SpeedResult =
      sessionMode === "page"
        ? { kind: "page", data: parsed }
        : sessionMode === "benchmark"
          ? { kind: "benchmark", data: parsed }
          : { kind: "single", data: parsed }
    setResult(speedResult)
    setUrl(loaded.session.url)
    setMode(sessionMode === "page" ? "page" : sessionMode === "benchmark" ? "benchmark" : "single")
  }, [])

  const deleteSession = useCallback(
    async (id: number) => {
      await deleteDownloadSpeedSession(id)
      await refreshSessions()
    },
    [refreshSessions],
  )

  const start = useCallback(async () => {
    reset()
    setIsRunning(true)
    try {
      if (mode === "single") {
        const finalResult = await runDownloadSpeedTest(url, httpSettings, (event) => {
          setProgress({ kind: "single", event })
        })
        const typedResult: SpeedResult = { kind: "single", data: finalResult }
        setResult(typedResult)
        await saveDownloadSpeedSession(url, mode, httpSettings, finalResult)
      } else if (mode === "page") {
        const finalResult = await runPageSpeedTest(url, httpSettings, (event) => {
          setProgress({ kind: "page", event })
        })
        const typedResult: SpeedResult = { kind: "page", data: finalResult }
        setResult(typedResult)
        await saveDownloadSpeedSession(url, mode, httpSettings, finalResult)
      } else {
        const config: WebBenchmarkConfig = {
          url,
          protocols: benchmarkConfig.protocols,
          runs: benchmarkConfig.runs,
          connectionMode: benchmarkConfig.connectionMode,
          concurrency: benchmarkConfig.concurrency,
          probe: benchmarkConfig.probe,
          httpSettings,
        }
        const finalResult = await runWebBenchmark(config)
        const typedResult: SpeedResult = { kind: "benchmark", data: finalResult }
        setResult(typedResult)
        await saveDownloadSpeedSession(url, mode, httpSettings, finalResult)
      }
      await refreshSessions()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setIsRunning(false)
    }
  }, [reset, url, mode, benchmarkConfig, httpSettings, refreshSessions])

  const updateBenchmarkConfig = useCallback((patch: Partial<BenchmarkConfigState>) => {
    setBenchmarkConfig((current) => ({ ...current, ...patch }))
  }, [])

  return {
    url,
    setUrl,
    mode,
    setMode,
    benchmarkConfig,
    updateBenchmarkConfig,
    resetBenchmarkConfig,
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
    sessions,
    sessionsLoading,
    refreshSessions,
    loadSession,
    deleteSession,
  }
}
