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

function SideValue({ value }: { readonly value: TraceHopRow | null }) {
  if (value === null) return <span className={styles.missing}>-</span>
  return (
    <>
      <div className={styles.address}>{value.address ?? "-"}</div>
      <div className={styles.hostname}>{value.hostname ?? "-"}</div>
      <div className={styles.rtts}>
        {formatRtt(value.rtt1Ms)} / {formatRtt(value.rtt2Ms)} /{" "}
        {formatRtt(value.rtt3Ms)}
      </div>
      <div className={styles.annotation}>{value.annotation ?? "-"}</div>
    </>
  )
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
                <th>Hop</th>
                <th>A</th>
                <th>B</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {diff.map((row) => (
                <tr
                  key={row.hop}
                  className={styles[row.status]}
                  data-testid="trace-comparison-row"
                >
                  <td>{row.hop}</td>
                  <td className={styles.sideA}>
                    <SideValue value={row.a} />
                  </td>
                  <td className={styles.sideB}>
                    <SideValue value={row.b} />
                  </td>
                  <td className={styles.status}>{formatStatus(row.status)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
