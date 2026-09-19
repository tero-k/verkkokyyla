import { formatMetric } from "../lib/format"
import type { MikrotikInterfaceDto } from "../lib/types"

import styles from "./MikrotikInterfaceTable.module.css"

type MikrotikInterfaceTableProps = {
  readonly interfaces: readonly MikrotikInterfaceDto[]
  readonly onSelectInterface: (name: string) => void
}

type CounterCellProps = {
  readonly field: string
  readonly value: number | null
}

function formatBitsPerSecond(value: number | null): string {
  if (value === null) return "-"
  if (value >= 1_000_000_000) {
    return `${formatMetric(value / 1_000_000_000)} Gbit/s`
  }
  if (value >= 1_000_000) {
    return `${formatMetric(value / 1_000_000)} Mbit/s`
  }
  if (value >= 1_000) {
    return `${formatMetric(value / 1_000)} Kbit/s`
  }
  return `${formatMetric(value, 0)} bit/s`
}

function formatBytes(value: number | null): string {
  if (value === null) return "-"
  if (value >= 1_099_511_627_776) {
    return `${formatMetric(value / 1_099_511_627_776)} TB`
  }
  if (value >= 1_073_741_824) {
    return `${formatMetric(value / 1_073_741_824)} GB`
  }
  if (value >= 1_048_576) {
    return `${formatMetric(value / 1_048_576)} MB`
  }
  if (value >= 1_024) {
    return `${formatMetric(value / 1_024)} KB`
  }
  return `${formatMetric(value, 0)} B`
}

function formatCount(value: number | null): string {
  return value === null ? "-" : value.toString()
}

function formatLink(networkInterface: MikrotikInterfaceDto): string {
  // Bonding masters carry a rate aggregated from their slave ports by the
  // backend; RouterOS reports their interface type as "bond". Other
  // non-ethernet types (vlan, bridge, ...) have none.
  const linkBearing =
    networkInterface.type === "ether" || networkInterface.type === "bond"
  if (!linkBearing || networkInterface.rate === null) {
    return "-"
  }
  if (networkInterface.fullDuplex === null) return networkInterface.rate
  const duplex = networkInterface.fullDuplex ? "full duplex" : "half duplex"
  return `${networkInterface.rate} · ${duplex}`
}

function StateBadge({
  running,
  disabled,
}: {
  readonly running: boolean | null
  readonly disabled: boolean | null
}) {
  if (disabled === true) {
    return <span className={`${styles.badge} ${styles.disabled}`}>Disabled</span>
  }
  if (running === true) {
    return <span className={`${styles.badge} ${styles.running}`}>Up</span>
  }
  return <span className={`${styles.badge} ${styles.down}`}>Down</span>
}

function CounterCell({ field, value }: CounterCellProps) {
  const className = `${styles.numeric}${value !== null && value > 0 ? ` ${styles.warning}` : ""}`
  return (
    <td className={className} data-testid={`counter-${field}`}>
      {formatCount(value)}
    </td>
  )
}

export function MikrotikInterfaceTable({
  interfaces,
  onSelectInterface,
}: MikrotikInterfaceTableProps) {
  return (
    <div className={styles.wrapper}>
      <table
        className={styles.table}
        aria-label="MikroTik interfaces"
        data-testid="mikrotik-interface-table"
      >
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Comment</th>
            <th scope="col">Type</th>
            <th scope="col">State</th>
            <th scope="col">Link rate / duplex</th>
            <th scope="col">RX rate</th>
            <th scope="col">TX rate</th>
            <th scope="col">RX bytes</th>
            <th scope="col">TX bytes</th>
            <th scope="col">RX packets</th>
            <th scope="col">TX packets</th>
            <th scope="col">TX queue drop</th>
            <th scope="col">TX drop</th>
            <th scope="col">Link downs</th>
            <th scope="col">RX error</th>
            <th scope="col">TX error</th>
            <th scope="col">RX drop</th>
            <th scope="col">RX error events</th>
            <th scope="col">TX error events</th>
            <th scope="col">RX FCS error</th>
            <th scope="col">TX collision</th>
          </tr>
        </thead>
        <tbody>
          {interfaces.map((networkInterface) => (
            <tr
              key={networkInterface.name}
              className={styles.selectableRow}
              data-testid={`interface-row-${networkInterface.name}`}
              onClick={() => onSelectInterface(networkInterface.name)}
            >
              <td>
                <button
                  type="button"
                  className={styles.interfaceButton}
                  aria-label={`Select interface ${networkInterface.name}`}
                >
                  {networkInterface.name}
                </button>
              </td>
              <td className={styles.secondary} title={networkInterface.comment ?? undefined}>
                {networkInterface.comment ?? "-"}
              </td>
              <td className={styles.secondary}>{networkInterface.type ?? "-"}</td>
              <td>
                <StateBadge
                  running={networkInterface.running}
                  disabled={networkInterface.disabled}
                />
              </td>
              <td data-testid="link">{formatLink(networkInterface)}</td>
              <td className={styles.numeric} data-testid="rx-rate">
                {formatBitsPerSecond(networkInterface.rxBitsPerSecond)}
              </td>
              <td className={styles.numeric} data-testid="tx-rate">
                {formatBitsPerSecond(networkInterface.txBitsPerSecond)}
              </td>
              <td className={styles.numeric} data-testid="rx-bytes">
                {formatBytes(networkInterface.rxByte)}
              </td>
              <td className={styles.numeric} data-testid="tx-bytes">
                {formatBytes(networkInterface.txByte)}
              </td>
              <td className={styles.numeric}>{formatCount(networkInterface.rxPacket)}</td>
              <td className={styles.numeric}>{formatCount(networkInterface.txPacket)}</td>
              <CounterCell field="tx-queue-drop" value={networkInterface.txQueueDrop} />
              <CounterCell field="tx-drop" value={networkInterface.txDrop} />
              <CounterCell field="link-downs" value={networkInterface.linkDowns} />
              <CounterCell field="rx-error" value={networkInterface.rxError} />
              <CounterCell field="tx-error" value={networkInterface.txError} />
              <CounterCell field="rx-drop" value={networkInterface.rxDrop} />
              <CounterCell field="rx-error-events" value={networkInterface.rxErrorEvents} />
              <CounterCell field="tx-error-events" value={networkInterface.txErrorEvents} />
              <CounterCell field="rx-fcs-error" value={networkInterface.rxFcsError} />
              <CounterCell field="tx-collision" value={networkInterface.txCollision} />
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
