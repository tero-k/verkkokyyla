import { useTraceroute } from "../hooks/useTraceroute"
import { FAMILIES, type Family } from "../lib/types"
import { TraceSessionPanel } from "../components/TraceSessionPanel"
import { TracerouteTable } from "../components/TracerouteTable"

import styles from "./TracerouteView.module.css"

const MAX_HOPS = 30

function familyLabel(family: Family): string {
  if (family === "auto") return "Auto"
  if (family === "v4") return "IPv4"
  return "IPv6"
}

export default function TracerouteView() {
  const {
    target,
    setTarget,
    family,
    setFamily,
    isRunning,
    error,
    status,
    hops,
    pastTraces,
    pastTrace,
    start,
    stop,
    openTrace,
    deleteTrace,
  } = useTraceroute()

  const canStart = target.trim().length > 0 && !isRunning
  const progressHop = isRunning ? hops.length : pastTrace?.trace.hopCount ?? hops.length

  return (
    <section className={styles.view} data-testid="traceroute-view">
      <header className={styles.header}>
        <h1>Traceroute</h1>
      </header>

      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor="trace-target">Target</label>
          <input
            id="trace-target"
            type="text"
            value={target}
            onChange={(event) => setTarget(event.target.value)}
            placeholder="example.com, 192.168.1.1, ::1"
            disabled={isRunning}
            data-testid="trace-target"
          />
        </div>

        <div className={styles.field}>
          <label htmlFor="trace-family">Family</label>
          <select
            id="trace-family"
            value={family}
            onChange={(event) => setFamily(event.target.value as Family)}
            disabled={isRunning}
            data-testid="trace-family"
          >
            {FAMILIES.map((value) => (
              <option key={value} value={value}>
                {familyLabel(value)}
              </option>
            ))}
          </select>
        </div>

        <div className={styles.actions}>
          <button
            type="button"
            onClick={() => void start()}
            disabled={!canStart}
            data-testid="trace-start"
          >
            Start
          </button>
          <button
            type="button"
            onClick={() => void stop()}
            disabled={!isRunning}
            data-testid="trace-stop"
          >
            Stop
          </button>
        </div>
      </div>

      {(status.length > 0 || error.length > 0) && (
        <div className={styles.banner}>
          {status.length > 0 && (
            <div className={styles.status} data-testid="trace-status">
              {status}
            </div>
          )}
          {error.length > 0 && (
            <div className={styles.error} data-testid="trace-error">
              {error}
            </div>
          )}
        </div>
      )}

      {(isRunning || hops.length > 0 || pastTrace !== null) && (
        <div className={styles.progress} data-testid="trace-progress">
          Hop {progressHop}/{MAX_HOPS}
        </div>
      )}

      <div className={styles.content}>
        <div className={styles.livePane}>
          <TracerouteTable rows={hops} />
        </div>
        <div className={styles.historyPane}>
          <TraceSessionPanel
            sessions={pastTraces}
            disabled={isRunning}
            onOpen={openTrace}
            onDelete={deleteTrace}
          />
        </div>
      </div>
    </section>
  )
}
