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
  ["Ctrl 8", "OPNsense"],
  ["Ctrl 9", "Help"],
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
            <strong>Discover</strong> (network reconnaissance), and <strong>Devices</strong>{" "}
            (router and firewall management). Every tool keeps a local history where applicable:
            sessions are stored in a SQLite database on this machine and can be reopened later.
          </p>
          <ul className={styles.shortcuts}>
            {SHORTCUTS.map(([kbd, label]) => (
              <li key={kbd}>
                <kbd>{kbd}</kbd> {label}
              </li>
            ))}
          </ul>
          <p>The theme switcher at the bottom of the sidebar toggles Light, Dark, and OS.</p>
          <p>
            At narrow window widths, wide tables get a horizontal slider below them so every
            column stays reachable; it only appears when a table actually overflows.
          </p>
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
            announced. Dismissing the notice hides that release, but a later
            stable release shows a new notice. The checkbox above turns the
            startup check off entirely; it never blocks downloading an
            installer manually.
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
            Measures HTTP throughput against a URL in three modes, chosen with the
            Download / Page load / Throughput selector. <strong>Download</strong> fetches a
            single file with live progress and final stats. <strong>Page load</strong> fetches
            a full page plus its resources and draws a per-request waterfall showing when each
            resource started, how long it took, and how fast it arrived.{" "}
            <strong>Throughput</strong> runs a repeatable detailed benchmark. The HTTP settings
            panel tunes protocol version, timeouts, redirects, and compression. Use it to
            compare links or verify that a path actually delivers its rated speed.
          </p>
        </section>

        <section className={styles.section}>
          <h2>Network scanner</h2>
          <p>
            Scans the local network and lists discovered hosts. Pick the network interface and
            the subnet to scan. Discovery uses ARP by default; an optional ARP + TCP fallback
            also probes hosts that stay silent on ARP. Enabling the top-25 TCP port probe shows
            a consent warning first, since port scans can trip network monitors and IDS tools.
            Results can be saved and reopened later, and <strong>Export CSV</strong> writes the
            host list through the native save dialog. Handy for finding a device&apos;s
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
            Five tabs around a configurable resolver. <strong>Lookup</strong> queries a domain
            for the selected record types (A, AAAA, MX, NS, SOA, TXT) against a resolver you
            set. <strong>Benchmark</strong> races resolvers with Quick, Stress, and Cache-bust
            presets to compare latency and caching behavior. <strong>Diagnostics</strong> runs
            a check series and shows the evidence chain, so you can tell whether a resolution
            problem is local, upstream, or at the authoritative server. <strong>Email</strong>{" "}
            validates SPF and DMARC and probes DKIM with your own selector list.{" "}
            <strong>History</strong> lists saved runs for reopening.
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

        <section className={styles.section}>
          <h2>OPNsense: profiles &amp; read-only API key</h2>
          <p>
            Add a profile per firewall: the host as a DNS name or IP without a scheme, the HTTPS
            port, a separately configurable SSH port for the terminal, plus an OPNsense API
            key/secret stored in the operating system keyring — never in the app&apos;s
            database. Use <strong>Test connection</strong> to distinguish &quot;the firewall is
            reachable&quot; from &quot;the API key is authorized&quot; before connecting. When
            editing a profile, leaving the key and secret blank keeps the stored credentials;
            changing the host or SSH port clears the pinned SSH host key so it is verified again.
            The key must be restricted to read-only (GET) access: the app only ever reads rule
            counters, status, VPN state, installed firmware and cached plugin upgrades, and
            saved revisions. If an
            OPNsense privilege bundles a needed read with write endpoints and no strictly
            read-only right exists, that section stays unavailable and the app says so instead of
            asking for elevated credentials. Backup downloads read already-saved revisions — the
            app never creates a remote snapshot, reverts, or deletes anything on the firewall.
          </p>
        </section>

        <section className={styles.section}>
          <h2>OPNsense: monitoring, rules &amp; logs</h2>
          <p>
            Select a profile and press Connect to start a live monitor; connected firewalls show
            as chips in the device strip, and switching the profile or chip switches the active
            firewall. <strong>Overview</strong> reports system resources and PF state-table
            usage, <strong>Firewall rules</strong> groups hit counters by interface, and{" "}
            <strong>Interfaces &amp; VPN</strong> covers link aggregation plus WireGuard,
            OpenVPN, IPsec, and the Tailscale plugin when installed. Each section reports
            success or failure independently — a failed section keeps its
            last good data with a stale timestamp instead of showing zeros. When OPNsense
            ignores GET pagination the affected list is marked incomplete rather than silently
            implying complete results; this read-only monitor will not use POST to fetch more.
            Rule counters are
            reported by OPNsense and reset on reload; older versions may omit legacy rules. The
            Firewall logs tab has its own Connect/Disconnect and streams pass/block decisions
            live, with All/Allowed/Blocked action filters and a text search over the visible
            fields — but only traffic matched by rules with logging enabled appears.
          </p>
        </section>

        <section className={styles.section}>
          <h2>OPNsense: firmware and plugin status</h2>
          <p>
            The Firmware tab&apos;s <strong>Read cached status</strong> compares the installed
            core version with the firewall&apos;s cached
            release changelog; it does not start a check or install updates. A missing or stale
            cache cannot establish current availability. Installed plugins show upgrade targets
            only if the firewall&apos;s previous check reported one; an absent target does not prove
            a plugin is current. Run &quot;Check for updates&quot; in the OPNsense GUI for fresh core
            and plugin availability.
          </p>
        </section>

        <section className={styles.section}>
          <h2>OPNsense: encrypted backups</h2>
          <p>
            Backups download the selected <em>saved</em> configuration revision (or the latest
            saved revision by default) — possibly older than unsaved changes on the device.
            Pick the destination with <strong>Choose directory</strong>, select the revision
            from the saved-revision list, and enter a passphrase of at least 12 characters
            spanning two of lowercase, uppercase, digit, and symbol classes. The file is
            encrypted locally in age format (<code>*.xml.age</code>) before it touches disk, and
            the passphrase is cleared from the form after each attempt, successful or not.
            To restore, decrypt outside the app with{" "}
            <code>age -d -o config.xml &lt;file&gt;.xml.age</code> and import the XML in the
            OPNsense GUI. The app never keeps plaintext XML on disk and never restores by itself.
          </p>
        </section>

        <section className={styles.section}>
          <h2>OPNsense: terminal</h2>
          <p>
            The Terminal tab opens an interactive SSH shell using a <strong>separate</strong>{" "}
            OPNsense user and password entered each time — the API key is never used for SSH, and
            SSH credentials never call the HTTP API. Commands run only inside that authenticated
            session. On the first connection the app shows the firewall&apos;s SSH host-key
            fingerprint and refuses to authenticate until you explicitly verify it; the verified
            fingerprint is pinned locally, a changed key is rejected, and re-keying requires
            deliberately forgetting the pinned key. As with MikroTik, the colored identity tag
            and matching device-strip pill show which firewall a shell drives.
          </p>
        </section>
      </div>
    </section>
  )
}
