import { ViewHeader } from "../components/ui/ui"

import styles from "./HelpView.module.css"

type HelpViewProps = {
  updateCheckEnabled: boolean
  onUpdateCheckEnabledChange: (enabled: boolean) => void
  appVersion: string
}

const SHORTCUTS: readonly (readonly [string, string])[] = [
  ["Ctrl 1", "Ping"],
  ["Ctrl 2", "Traceroute"],
  ["Ctrl 3", "Web Benchmark"],
  ["Ctrl 4", "Network scanner"],
  ["Ctrl 5", "MTU Discovery"],
  ["Ctrl 6", "DNS Toolkit"],
  ["Ctrl 7", "MikroTik"],
  ["Ctrl 8", "Help"],
]

export default function HelpView({
  updateCheckEnabled,
  onUpdateCheckEnabledChange,
  appVersion,
}: HelpViewProps) {
  return (
    <section className={styles.view} data-testid="help-view">
      <ViewHeader
        title={<h1>Help &amp; how to</h1>}
        subtitle="what each tool does and how to use it"
      />
      <div className={styles.sections}>
        <section className={styles.section}>
          <h2>Getting around</h2>
          <p>
            Tools are grouped in the sidebar: <strong>Measure</strong> (active probes),{" "}
            <strong>Discover</strong> (network reconnaissance), and <strong>MikroTik</strong>{" "}
            (router management). Every tool keeps a local history: sessions are stored in a
            SQLite database on this machine and can be reopened later.
          </p>
          <ul className={styles.shortcuts}>
            {SHORTCUTS.map(([kbd, label]) => (
              <li key={kbd}>
                <kbd>{kbd}</kbd> {label}
              </li>
            ))}
          </ul>
          <p>The theme switcher at the bottom of the sidebar toggles Light, Dark, and OS.</p>
        </section>

        <section className={styles.section}>
          <h2>Updates</h2>
          <p>
            <label className={styles.toggleRow}>
              <input
                type="checkbox"
                checked={updateCheckEnabled}
                onChange={(e) => onUpdateCheckEnabledChange(e.target.checked)}
                data-testid="update-check-toggle"
              />
              Check for updates on startup
            </label>
          </p>
          <p>
            When a newer stable release exists, a notice appears at the top of
            the window with a link to the release page — the app never
            downloads or installs updates on its own. Prereleases are never
            announced.
            {appVersion ? ` Running version v${appVersion}.` : ""}
          </p>
        </section>

        <section className={styles.section}>
          <h2>Ping / ICMP</h2>
          <p>
            Enter a host and start a session: results stream into the live table (the latest 500
            rows) and the latency graph. Past sessions are listed on the view and can be reopened.
            The ping engine is chosen automatically for the platform.
          </p>
        </section>

        <section className={styles.section}>
          <h2>Traceroute</h2>
          <p>
            Runs the operating system&apos;s native traceroute and streams hops in as they
            arrive. Traces are kept in history just like ping sessions.
          </p>
        </section>

        <section className={styles.section}>
          <h2>Web Benchmark</h2>
          <p>
            Downloads a URL and measures throughput with live progress and final stats. Use it to
            compare links or verify that a path actually delivers its rated speed.
          </p>
        </section>

        <section className={styles.section}>
          <h2>Network scanner</h2>
          <p>
            Scans the local network and lists discovered hosts. Handy for finding a device&apos;s
            address before adding it as a ping target or MikroTik profile.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MTU Discovery</h2>
          <p>
            Finds the path MTU with Don&apos;t-Fragment ICMP probes (bracketing steps first, then
            a binary search). On Linux it falls back to TCP probing when ICMP is filtered. Results
            stream live and are kept in history.
          </p>
        </section>

        <section className={styles.section}>
          <h2>DNS Toolkit</h2>
          <p>
            Runs DNS diagnostics against resolvers and shows the evidence chain, so you can tell
            whether a resolution problem is local, upstream, or at the authoritative server.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MikroTik: profiles &amp; credentials</h2>
          <p>
            Add a profile per router: host, REST API port, and credentials. RouterOS v7.1 or newer
            with the <strong>www-ssl</strong> service is required; plain HTTP works on v7.9+ with{" "}
            <strong>www</strong>. The password is stored in the operating system keyring, never
            in the app&apos;s database.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MikroTik: monitoring</h2>
          <p>
            Connect a profile to start a live session: resources, interface rates, VLANs, and
            version/firmware status update continuously. Devices monitor independently (one
            session per device, up to eight at once) and keep collecting while you work in other
            tools. Stopped sessions are saved to history and can be reloaded.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MikroTik: logs</h2>
          <p>
            The Logs tab streams the device&apos;s log with severity highlighting and a text
            filter. Streams run per device and, like monitoring, keep collecting in the
            background.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MikroTik: terminal</h2>
          <p>
            The Terminal tab opens an interactive SSH shell, one per device. Shells stay open when
            you switch devices or tools. The colored <strong>identity tag</strong> at the top of
            each terminal (and the matching <strong>term</strong> pill on the device chip) shows
            which router you are typing into. Switching to a different device&apos;s terminal
            shows a warning naming the connection; it can be silenced for the rest of the session.
          </p>
        </section>

        <section className={styles.section}>
          <h2>MikroTik: backups</h2>
          <p>
            Creates a backup on the router, downloads it over SFTP, and removes the temporary
            file. The router&apos;s SSH service must be enabled. Backups with the .rsc export can
            be compared in the library: select two to see a line-by-line config diff.
          </p>
        </section>

        <section className={styles.section}>
          <h2>Security note</h2>
          <p>
            For typical LAN-managed routers the app accepts unknown SSH host keys on first
            connection instead of requiring a known-hosts entry. If you manage devices you cannot
            physically trust, verify fingerprints out of band before opening terminals or backups.
          </p>
        </section>
      </div>
    </section>
  )
}
