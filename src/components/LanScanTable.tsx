import type { ScanHostDto } from "../lib/types"

import styles from "./LanScanTable.module.css"

type LanScanTableProps = {
  readonly rows: readonly ScanHostDto[]
}

function formatNullable(value: string | null): string {
  return value ?? "-"
}

function formatPorts(ports: readonly { readonly port: number; readonly service: string }[]): string {
  return ports.map((p) => `${p.port} ${p.service}`).join(", ") || "-"
}

export function LanScanTable({ rows }: LanScanTableProps) {
  return (
    <div className={styles.wrapper}>
      <table className={styles.table} data-testid="lan-scan-table">
        <thead>
          <tr>
            <th>IP</th>
            <th>MAC</th>
            <th>Vendor</th>
            <th>Hostname</th>
            <th>Ports</th>
            <th>Found by</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.ip} data-testid="lan-scan-row">
              <td>{row.ip}</td>
              <td>{formatNullable(row.mac)}</td>
              <td>{formatNullable(row.vendor)}</td>
            <td>{formatNullable(row.hostname)}</td>
            <td title={formatPorts(row.openPorts)}>{formatPorts(row.openPorts)}</td>
            <td>{row.foundBy}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
