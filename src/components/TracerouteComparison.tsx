import type { ComparedHopRow, LoadedTraceDto, TraceHopRow } from "../lib/types"

import styles from "./TracerouteComparison.module.css"

type TracerouteComparisonProps = {
  readonly a: LoadedTraceDto
  readonly b: LoadedTraceDto
  readonly diff: readonly ComparedHopRow[]
  readonly onClear: () => void
}

function formatRtt(value: number | null): string {
  if (value === null) return "-"
  return Number.isInteger(value) ? `${value} ms` : `${value.toFixed(2)} ms`
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

function formatStatus(status: ComparedHopRow["status"]): string {
  switch (status) {
    case "same":
      return "Same"
    case "changed":
      return "Changed"
    case "a-only":
      return "Only A"
    case "b-only":
      return "Only B"
  }
}

function formatInline(hop: TraceHopRow | null): string {
  if (hop === null) return "-"
  const parts: string[] = []
  if (hop.address) parts.push(hop.address)
  if (hop.hostname) parts.push(`(${hop.hostname})`)
  parts.push(
    [hop.rtt1Ms, hop.rtt2Ms, hop.rtt3Ms].map(formatRtt).join(" / "),
  )
  if (hop.annotation) parts.push(hop.annotation)
  return parts.join(" · ")
}

export function TracerouteComparison({ a, b, diff, onClear }: TracerouteComparisonProps) {
  return (
    <div className={styles.wrapper} data-testid="trace-comparison-view">
      <div className={styles.header}>
        <div className={styles.title}>
          Comparing trace #{a.trace.id} ({a.trace.targetInput}, {a.trace.resolvedIp},{" "}
          {formatDateTime(a.trace.startedAt)}) vs trace #{b.trace.id} ({b.trace.targetInput},{" "}
          {b.trace.resolvedIp}, {formatDateTime(b.trace.startedAt)})
        </div>
        <button type="button" onClick={onClear} data-testid="trace-comparison-clear">
          Back
        </button>
      </div>

      {diff.length === 0 ? (
        <p className={styles.empty}>No hops to compare.</p>
      ) : (
        <div className={styles.tableWrapper}>
          <table className={styles.table} data-testid="trace-comparison-table">
            <thead>
              <tr>
                <th className={styles.hopCol}>Hop</th>
                <th className={styles.sideCol}>A</th>
                <th className={styles.sideCol}>B</th>
                <th className={styles.statusCol}>Status</th>
              </tr>
            </thead>
            <tbody>
              {diff.map((row) => (
                <tr
                  key={row.hop}
                  className={styles[row.status]}
                  data-testid="trace-comparison-row"
                >
                  <td className={styles.hopCol}>{row.hop}</td>
                  <td className={styles.sideCol}>
                    <span className={styles.inline}>{formatInline(row.a)}</span>
                  </td>
                  <td className={styles.sideCol}>
                    <span className={styles.inline}>{formatInline(row.b)}</span>
                  </td>
                  <td className={styles.statusCol}>{formatStatus(row.status)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
