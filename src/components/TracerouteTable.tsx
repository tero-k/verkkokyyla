import type { TraceHopRow } from "../lib/types"
import { Card } from "./ui/ui"

import styles from "./TracerouteTable.module.css"

type TracerouteTableProps = {
  readonly rows: readonly TraceHopRow[]
}

type RttStats = {
  readonly min: number
  readonly average: number
  readonly max: number
}

function probeValues(row: TraceHopRow): readonly (number | null)[] {
  return [row.rtt1Ms, row.rtt2Ms, row.rtt3Ms]
}

function rttStats(values: readonly (number | null)[]): RttStats | null {
  const responses = values.filter((value): value is number => value !== null)
  if (responses.length === 0) return null
  return {
    min: Math.min(...responses),
    average: responses.reduce((sum, value) => sum + value, 0) / responses.length,
    max: Math.max(...responses),
  }
}

function formatRtt(value: number): string {
  return value.toFixed(value < 10 ? 2 : 1)
}

function formatRttStats(stats: RttStats | null): string {
  if (stats === null) return "—"
  return `${formatRtt(stats.min)} · ${formatRtt(stats.average)} · ${formatRtt(stats.max)}`
}

function formatLoss(values: readonly (number | null)[]): string {
  const lost = values.filter((value) => value === null).length
  return `${Math.round((lost / values.length) * 100)} %`
}

function probeClass(value: number | null): string {
  if (value === null) return styles.probeMiss
  if (value <= 5) return styles.probeCool
  if (value <= 15) return styles.probeMild
  if (value <= 30) return styles.probeWarm
  if (value <= 60) return styles.probeHot
  return styles.probePeak
}

function lossClass(values: readonly (number | null)[]): string {
  const lost = values.filter((value) => value === null).length
  if (lost === 0) return styles.lossClear
  if (lost === values.length) return styles.lossFull
  return styles.lossPartial
}

export function TracerouteTable({ rows }: TracerouteTableProps) {
  return (
    <Card className={styles.wrapper}>
      <table className={styles.table} data-testid="trace-table">
        <thead>
          <tr>
            <th scope="col" className={styles.hopColumn}>Hop</th>
            <th scope="col" className={styles.hostColumn}>Host</th>
            <th scope="col" className={styles.networkColumn}>Network / AS</th>
            <th scope="col" className={styles.probesColumn}>Probes</th>
            <th scope="col" className={styles.timingColumn}>Min · Avg · Max</th>
            <th scope="col" className={styles.lossColumn}>Loss</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && (
            <tr className={styles.emptyRow}>
              <td colSpan={6}>No route data yet. Enter a target and run a trace.</td>
            </tr>
          )}
          {rows.map((row, rowIndex) => {
            const values = probeValues(row)
            const stats = rttStats(values)
            const noResponse = row.address === null
            const hostName = row.hostname ?? (noResponse ? "* * *" : "Resolving host…")
            const hostClass = [
              styles.hostName,
              noResponse ? styles.noResponse : "",
              rowIndex === rows.length - 1 && !noResponse ? styles.destination : "",
            ]
              .filter(Boolean)
              .join(" ")

            return (
              <tr key={row.hop} data-testid="trace-row">
                <td className={styles.hopCell}>
                  <span className={styles.hopRail} />
                  <span className={styles.hopNumber}>{row.hop}</span>
                </td>
                <td className={styles.hostCell}>
                  <span className={hostClass}>{hostName}</span>
                  <span className={styles.hostAddress}>
                    {row.address ?? "no response (filtered)"}
                  </span>
                </td>
                <td className={styles.networkCell}>{row.annotation ?? "—"}</td>
                <td className={styles.probesCell}>
                  <div className={styles.probes} aria-label={`Probe times for hop ${row.hop}`}>
                    {values.map((value, probeIndex) => (
                      <span
                        key={probeIndex}
                        className={`${styles.probe} ${probeClass(value)}`}
                        title={value === null ? "No response" : `${formatRtt(value)} ms`}
                      />
                    ))}
                  </div>
                </td>
                <td className={styles.timingCell}>
                  <span>{formatRttStats(stats)}</span>
                  {stats !== null && <span className={styles.unit}>ms</span>}
                </td>
                <td className={`${styles.lossCell} ${lossClass(values)}`}>
                  {formatLoss(values)}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </Card>
  )
}
