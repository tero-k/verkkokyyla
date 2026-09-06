import type { MikrotikBridgeVlanDto, MikrotikVlanDto } from "../lib/types"

import styles from "./MikrotikVlanPanel.module.css"

type MikrotikVlanPanelProps = {
  readonly vlans: readonly MikrotikVlanDto[]
  readonly bridgeVlans: readonly MikrotikBridgeVlanDto[]
}

function formatList(values: readonly string[]): string {
  return values.length === 0 ? "-" : values.join(", ")
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
    return <span className={`${styles.badge} ${styles.running}`}>Running</span>
  }
  return <span className={`${styles.badge} ${styles.down}`}>Down</span>
}

export function MikrotikVlanPanel({
  vlans,
  bridgeVlans,
}: MikrotikVlanPanelProps) {
  return (
    <div className={styles.panel} data-testid="mikrotik-vlan-panel">
      <section className={styles.section} aria-label="VLAN interface configuration">
        <h2>VLAN interfaces</h2>
        {vlans.length === 0 ? (
          <p className={styles.empty}>No VLANs configured</p>
        ) : (
          <div className={styles.tableWrapper}>
            <table className={styles.table} aria-label="VLAN interfaces">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">VLAN ID</th>
                  <th scope="col">Parent interface</th>
                  <th scope="col">State</th>
                </tr>
              </thead>
              <tbody>
                {vlans.map((vlan) => (
                  <tr key={vlan.name} data-testid="vlan-interface-row">
                    <td className={styles.primary}>{vlan.name}</td>
                    <td className={styles.numeric}>{vlan.vlanId ?? "-"}</td>
                    <td className={styles.secondary}>{vlan.interface ?? "-"}</td>
                    <td>
                      <StateBadge running={vlan.running} disabled={vlan.disabled} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className={styles.section} aria-label="Bridge VLAN configuration">
        <h2>Bridge VLAN entries</h2>
        {bridgeVlans.length === 0 ? (
          <p className={styles.empty}>No VLANs configured</p>
        ) : (
          <div className={styles.tableWrapper}>
            <table className={styles.table} aria-label="Bridge VLAN entries">
              <thead>
                <tr>
                  <th scope="col">Bridge</th>
                  <th scope="col">VLAN IDs</th>
                  <th scope="col">Tagged</th>
                  <th scope="col">Untagged</th>
                  <th scope="col">Current tagged</th>
                  <th scope="col">Current untagged</th>
                </tr>
              </thead>
              <tbody>
                {bridgeVlans.map((entry) => (
                  <tr
                    key={`${entry.bridge ?? "-"}:${entry.vlanIds.join(",")}`}
                    data-testid="bridge-vlan-row"
                  >
                    <td className={styles.primary}>{entry.bridge ?? "-"}</td>
                    <td className={styles.numeric}>{formatList(entry.vlanIds)}</td>
                    <td>{formatList(entry.tagged)}</td>
                    <td>{formatList(entry.untagged)}</td>
                    <td>{formatList(entry.currentTagged)}</td>
                    <td>{formatList(entry.currentUntagged)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
