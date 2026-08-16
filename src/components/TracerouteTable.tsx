import type { TraceHopRow } from "../lib/types"

import styles from "./TracerouteTable.module.css"

type TracerouteTableProps = {
  readonly rows: readonly TraceHopRow[]
}

function formatRtt(value: number | null): string {
  if (value === null) return "-"
  return Number.isInteger(value) ? `${value} ms` : `${value.toFixed(2)} ms`
}

export function TracerouteTable({ rows }: TracerouteTableProps) {
  return (
    <div className={styles.wrapper}>
      <table className={styles.table} data-testid="trace-table">
        <thead>
          <tr>
            <th>Hop</th>
            <th>Address</th>
            <th>Hostname</th>
            <th>RTT1</th>
            <th>RTT2</th>
            <th>RTT3</th>
            <th>Note</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.hop} data-testid="trace-row">
              <td>{row.hop}</td>
              <td>{row.address ?? "-"}</td>
              <td>{row.hostname ?? "-"}</td>
              <td>{formatRtt(row.rtt1Ms)}</td>
              <td>{formatRtt(row.rtt2Ms)}</td>
              <td>{formatRtt(row.rtt3Ms)}</td>
              <td>{row.annotation ?? "-"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
