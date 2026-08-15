import { useEffect, useRef, useState } from "react"
import { formatMetric, formatTime } from "../lib/format"
import type { ProbeRow, SnapshotDto } from "../lib/types"

import styles from "./PingStatsTable.module.css"

const COLLAPSED_ROWS = 5

type PingStatsTableProps = {
  readonly rows: readonly ProbeRow[]
  readonly snapshot: SnapshotDto | null
}

export function PingStatsTable({ rows, snapshot }: PingStatsTableProps) {
  const [expanded, setExpanded] = useState(false)
  const bodyRef = useRef<HTMLTableSectionElement>(null)

  const visibleRows = expanded ? rows : rows.slice(-COLLAPSED_ROWS)

  useEffect(() => {
    const body = bodyRef.current
    if (body === null) return
    body.scrollTop = body.scrollHeight
  }, [visibleRows])

  const lossPercent =
    snapshot !== null ? snapshot.lossFraction * 100 : null

  return (
    <div className={styles.wrapper}>
      <div className={styles.bar} data-testid="aggregates-bar">
        <div className={styles.metric}>
          <span className={styles.metricLabel}>count</span>
          <span className={styles.metricValue}>
            {snapshot?.count ?? "-"}
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>loss%</span>
          <span className={styles.metricValue}>
            {formatMetric(lossPercent, 2)}
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>min</span>
          <span className={styles.metricValue}>
            {formatMetric(snapshot?.minMs ?? null, 2)} ms
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>avg</span>
          <span className={styles.metricValue}>
            {formatMetric(snapshot?.avgMs ?? null, 2)} ms
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>max</span>
          <span className={styles.metricValue}>
            {formatMetric(snapshot?.maxMs ?? null, 2)} ms
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>stddev</span>
          <span className={styles.metricValue}>
            {formatMetric(snapshot?.stddevMs ?? null, 2)} ms
          </span>
        </div>
        <div className={styles.metric}>
          <span className={styles.metricLabel}>jitter</span>
          <span className={styles.metricValue}>
            {formatMetric(snapshot?.jitterMs ?? null, 2)} ms
          </span>
        </div>
      </div>

      {rows.length > COLLAPSED_ROWS && (
        <div className={styles.toggle}>
          <button
            type="button"
            onClick={() => setExpanded((prev) => !prev)}
            data-testid="expand-table"
          >
            {expanded ? "Show fewer" : `Show all (${rows.length})`}
          </button>
        </div>
      )}

      <div className={styles.tableWrapper}>
        <table className={styles.table}>
          <thead>
            <tr>
              <th>seq</th>
              <th>time</th>
              <th>rtt</th>
            </tr>
          </thead>
          <tbody ref={bodyRef}>
            {visibleRows.map((row) => (
              <tr
                key={row.seq}
                className={row.lost ? styles.lost : undefined}
                data-testid="ping-table-row"
                data-seq={row.seq}
              >
                <td>{row.seq}</td>
                <td>{formatTime(row.at)}</td>
                <td>
                  {row.lost || row.rttMs === null
                    ? "lost"
                    : `${row.rttMs.toFixed(2)} ms`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
