import type { ScanHostDto } from "../lib/types"

import styles from "./LanScanTable.module.css"

const SENSITIVE_SERVICES: ReadonlySet<string> = new Set([
  "ssh",
  "rdp",
  "ipmi",
  "winbox",
])

type LanScanTableProps = {
  readonly rows: readonly ScanHostDto[]
}

function formatNullable(value: string | null): string {
  return value ?? "-"
}

function formatPorts(ports: readonly { readonly port: number; readonly service: string }[]): string {
  return ports.map((p) => `${p.port} ${p.service}`).join(", ") || "-"
}

function formatLastSeen(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  })
}

export function LanScanTable({ rows }: LanScanTableProps) {
  return (
    <div className={styles.wrapper}>
      <table
        className={styles.table}
        data-testid="lan-scan-table"
        aria-label="Discovered network devices"
      >
        <colgroup>
          <col className={styles.ipColumn} />
          <col className={styles.macColumn} />
          <col className={styles.vendorColumn} />
          <col className={styles.hostnameColumn} />
          <col className={styles.portsColumn} />
          <col className={styles.rttColumn} />
          <col className={styles.seenColumn} />
        </colgroup>
        <thead>
          <tr>
            <th scope="col" className={styles.sortedColumn}>
              IP <span aria-hidden="true">▲</span>
            </th>
            <th scope="col">MAC</th>
            <th scope="col">Vendor</th>
            <th scope="col">Hostname</th>
            <th scope="col">Open ports</th>
            <th scope="col" className={styles.numeric}>RTT</th>
            <th scope="col" className={styles.numeric}>Last seen</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr className={styles.emptyRow}>
              <td colSpan={7}>No hosts discovered yet.</td>
            </tr>
          ) : (
            rows.map((row, index) => (
              <tr
                key={row.ip}
                className={index === rows.length - 1 ? styles.newDeviceRow : undefined}
                data-testid="lan-scan-row"
              >
                <td className={styles.ip} title={`Discovered by ${row.foundBy}`}>
                  {row.ip}
                </td>
                <td className={styles.mac}>{formatNullable(row.mac)}</td>
                <td className={styles.vendor}>{formatNullable(row.vendor)}</td>
                <td className={styles.hostname}>{formatNullable(row.hostname)}</td>
                <td className={styles.portsCell} title={formatPorts(row.openPorts)}>
                  <div className={styles.ports}>
                    {row.openPorts.length === 0 ? (
                      <span className={styles.emptyValue}>-</span>
                    ) : (
                      row.openPorts.map((port) => (
                        <span
                          key={`${port.port}:${port.service}`}
                          className={`${styles.portChip}${
                            SENSITIVE_SERVICES.has(port.service.toLowerCase())
                              ? ` ${styles.sensitivePort}`
                              : ""
                          }`}
                        >
                          {port.port} {port.service}
                        </span>
                      ))
                    )}
                  </div>
                </td>
                <td
                  className={`${styles.rtt} ${styles.numeric}`}
                  title="RTT is not reported by this scanner"
                >
                  -
                </td>
                <td className={`${styles.lastSeen} ${styles.numeric}`}>
                  <time dateTime={row.at}>{formatLastSeen(row.at)}</time>
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  )
}
