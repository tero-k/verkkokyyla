import { useEffect, useRef, useState } from "react"
import { formatMetric, formatTime } from "../lib/format"
import type { ProbeRow, SnapshotDto } from "../lib/types"
import { Button, Card, SectionHeader, Stat } from "./ui/ui"

import styles from "./PingStatsTable.module.css"

const COLLAPSED_ROWS = 5
const HEAT_PROBE_LIMIT = 48
const HEAT_MIN_OPACITY = 0.16
const HEAT_OPACITY_RANGE = 0.84

type PingStatsTableProps = {
  readonly rows: readonly ProbeRow[]
  readonly snapshot: SnapshotDto | null
}

export function PingStatsTable({ rows, snapshot }: PingStatsTableProps) {
  const [expanded, setExpanded] = useState(false)
  const bodyRef = useRef<HTMLTableSectionElement>(null)

  const visibleRows = expanded ? rows : rows.slice(-COLLAPSED_ROWS)
  const heatRows = rows.slice(-HEAT_PROBE_LIMIT)
  const maxLatency = heatRows.reduce(
    (currentMax, row) =>
      row.rttMs !== null && !row.lost
        ? Math.max(currentMax, row.rttMs)
        : currentMax,
    0,
  )

  useEffect(() => {
    const body = bodyRef.current
    if (body === null) return
    body.scrollTop = body.scrollHeight
  }, [visibleRows])

  const lossPercent =
    snapshot !== null ? snapshot.lossFraction * 100 : null

  return (
    <div className={styles.wrapper}>
      <Card className={styles.heatCard}>
        <div className={styles.heatHeader}>
          <SectionHeader
            title={`Latency heat strip — last ${HEAT_PROBE_LIMIT} probes`}
            aside={
              <span className={styles.legend}>
                <span>0 ms</span>
                <span className={styles.legendScale} aria-hidden="true" />
                <span>slow</span>
                <span className={styles.lossSwatch} aria-hidden="true" />
                <span>loss</span>
              </span>
            }
          />
        </div>

        {heatRows.length === 0 ? (
          <div className={styles.heatEmpty}>Waiting for probes</div>
        ) : (
          <div
            className={styles.heatStrip}
            aria-label={`Latency for the latest ${heatRows.length} probes`}
          >
            {heatRows.map((row, index) => {
              const opacity =
                row.rttMs === null || maxLatency === 0
                  ? HEAT_MIN_OPACITY
                  : HEAT_MIN_OPACITY +
                    (row.rttMs / maxLatency) * HEAT_OPACITY_RANGE
              return (
                <span
                  key={row.seq}
                  className={`${styles.heatCell}${row.lost ? ` ${styles.heatLost}` : ""}`}
                  style={
                    row.lost
                      ? { animationDelay: `${index * 12}ms` }
                      : {
                          animationDelay: `${index * 12}ms`,
                          opacity,
                        }
                  }
                  title={
                    row.lost || row.rttMs === null
                      ? `Probe ${row.seq}: lost`
                      : `Probe ${row.seq}: ${row.rttMs.toFixed(2)} ms`
                  }
                />
              )
            })}
          </div>
        )}

        <div className={styles.heatTimeline} aria-hidden="true">
          <span>−48 s</span>
          <span>−24 s</span>
          <span>now</span>
        </div>

        <div className={styles.bar} data-testid="aggregates-bar">
          <Stat
            label="Min"
            value={formatMetric(snapshot?.minMs ?? null, 2)}
            unit="ms"
            color="var(--accent)"
          />
          <Stat
            label="Avg"
            value={formatMetric(snapshot?.avgMs ?? null, 2)}
            unit="ms"
          />
          <Stat
            label="Max"
            value={formatMetric(snapshot?.maxMs ?? null, 2)}
            unit="ms"
            color="var(--warning)"
          />
          <Stat
            label="Mdev"
            value={formatMetric(snapshot?.stddevMs ?? null, 2)}
            unit="ms"
            color="var(--warning)"
          />
          <Stat
            label="Loss"
            value={formatMetric(lossPercent, 2)}
            unit="%"
            color="var(--danger)"
          />
          <Stat label="Sent" value={snapshot?.count ?? "-"} />
        </div>
      </Card>

      <Card className={styles.logCard}>
        <div className={styles.logHeader}>
          <SectionHeader
            title="Packet log"
            aside={
              <span className={styles.tableTools}>
                <span>auto-scroll</span>
                {rows.length > COLLAPSED_ROWS && (
                  <Button
                    small
                    className={styles.toggleButton}
                    onClick={() => setExpanded((previous) => !previous)}
                    data-testid="expand-table"
                  >
                    {expanded ? "Show fewer" : `Show all (${rows.length})`}
                  </Button>
                )}
              </span>
            }
          />
        </div>

        <div className={styles.tableWrapper}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th>seq</th>
                <th>time</th>
                <th>result</th>
                <th>rtt</th>
              </tr>
            </thead>
            <tbody ref={bodyRef}>
              {visibleRows.length === 0 ? (
                <tr className={styles.emptyRow}>
                  <td colSpan={4}>No packets recorded yet.</td>
                </tr>
              ) : (
                visibleRows.map((row) => (
                  <tr
                    key={row.seq}
                    className={row.lost ? styles.lost : undefined}
                    data-testid="ping-table-row"
                    data-seq={row.seq}
                  >
                    <td>{row.seq}</td>
                    <td>{formatTime(row.at)}</td>
                    <td className={styles.result}>
                      {row.lost || row.rttMs === null ? "lost" : "reply"}
                    </td>
                    <td className={styles.rtt}>
                      {row.lost || row.rttMs === null
                        ? "—"
                        : `${row.rttMs.toFixed(2)} ms`}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  )
}
