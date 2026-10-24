import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { Button, Card, Live, Segmented, ViewHeader } from "../components/ui/ui"
import { HorizontalScrollArea } from "../components/ui/HorizontalScrollArea"
import { SshTerminalPanel, terminalAccentFor, type SshTerminalAdapter } from "../components/SshTerminalPanel"
import { TerminalSwitchWarning } from "../components/TerminalSwitchWarning"
import { ConfirmDialog } from "../components/ConfirmDialog"
import { useOpnsense, type OpnsenseDeviceState, type OpnsenseSectionView, type RunOpnsenseBackupRequest } from "../hooks/useOpnsense"
import { useOpnsenseLogs, type OpnsenseLogTab } from "../hooks/useOpnsenseLogs"
import {
  opnsenseForgetSshHostKey,
  opnsenseTerminalClose,
  opnsenseTerminalOpen,
  opnsenseTerminalResize,
  opnsenseTerminalWrite,
} from "../lib/ipc"
import {
  opnsenseErrorPayload,
  type CreateOpnsenseProfileRequest,
  type OpnsenseBackupResultDto,
  type OpnsenseInterfaceDto,
  type OpnsenseProfile,
  type OpnsenseRevisionDto,
  type OpnsenseRuleDto,
  type OpnsenseVersionCheckDto,
  type OpnsenseVpnServiceDto,
  type OpnsenseWireguardPeerDto,
  type UpdateOpnsenseProfileRequest,
} from "../lib/types"

import styles from "./OpnsenseView.module.css"

const TABS = [
  { id: "profiles", label: "Profiles" },
  { id: "overview", label: "Overview" },
  { id: "rules", label: "Firewall rules" },
  { id: "vpn", label: "Interfaces & VPN" },
  { id: "logs", label: "Firewall logs" },
  { id: "firmware", label: "Firmware" },
  { id: "terminal", label: "Terminal" },
  { id: "backups", label: "Backups" },
] as const

type TabId = (typeof TABS)[number]["id"]

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

function dash(value: number | null | undefined): string {
  return value === null || value === undefined ? "—" : value.toLocaleString()
}

function formatBytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—"
  if (value < 1024) return `${value} B`
  const units = ["KiB", "MiB", "GiB", "TiB"]
  let amount = value / 1024
  let unit = 0
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024
    unit += 1
  }
  return `${amount.toFixed(1)} ${units[unit]}`
}

function formatHandshakeAge(secs: number): string {
  if (secs < 60) return `${secs}s ago`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`
  return `${Math.floor(secs / 86400)}d ago`
}

function actionBadgeClass(action: string): string {
  if (action === "pass") return styles.badgeOk
  if (action === "block" || action === "reject") return styles.badgeErr
  return styles.badgeMuted
}

const PASSPHRASE_HINT = "Use at least 12 characters with a mix of letters, digits or symbols."

/** Mirrors the backend policy: ≥12 chars AND ≥2 of lower/upper/digit/symbol. */
function passphraseStrength(passphrase: string): "blank" | "weak" | "ok" {
  if (passphrase.trim() === "") return "blank"
  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^\p{L}\p{N}]/u]
    .filter((re) => re.test(passphrase)).length
  return Array.from(passphrase).length >= 12 && classes >= 2 ? "ok" : "weak"
}

function staleTag(view: OpnsenseSectionView<unknown> | null | undefined): ReactNode {
  if (view == null || !view.stale) return null
  return (
    <span className={styles.staleTag} title="The last refresh of this section failed — showing the last good data.">
      stale{view.updatedAt !== null ? ` · last ok ${view.updatedAt}` : ""}
    </span>
  )
}

function sectionPlaceholder(view: OpnsenseSectionView<unknown> | null | undefined, connectHint: string): ReactNode {
  if (view === null || view === undefined) {
    return <p className={styles.hint}>{connectHint}</p>
  }
  if (view.data === null) {
    return <p className={styles.error}>{view.error?.message ?? "Not collected yet."}</p>
  }
  return null
}

function sectionRefreshNotice(view: OpnsenseSectionView<unknown> | null | undefined): ReactNode {
  if (view == null || view.data === null || view.error == null) return null
  return (
    <p className={`${styles.banner} ${styles.bannerWarn}`} role="status">
      Latest refresh failed: {view.error.message}. Showing data from{" "}
      {view.updatedAt ?? "an earlier check"}.
    </p>
  )
}

// ---------------------------------------------------------------------------
// Profiles tab
// ---------------------------------------------------------------------------

const emptyProfileForm = {
  id: null as number | null,
  name: "",
  host: "",
  port: 443,
  allowInvalidCerts: false,
  sshPort: 22,
  apiKey: "",
  apiSecret: "",
}

type ProfileFormState = typeof emptyProfileForm

type ProfilePanelProps = {
  readonly profiles: readonly OpnsenseProfile[]
  readonly onCreate: (request: CreateOpnsenseProfileRequest) => Promise<OpnsenseProfile | null>
  readonly onUpdate: (request: UpdateOpnsenseProfileRequest) => Promise<OpnsenseProfile | null>
  readonly onDelete: (id: number) => Promise<boolean>
  readonly onSetCredentials: (id: number, key: string, secret: string) => Promise<boolean>
  readonly onTestConnection: (id: number) => Promise<{ connected: boolean; authorized: boolean } | null>
}

function ProfilePanel({ profiles, onCreate, onUpdate, onDelete, onSetCredentials, onTestConnection }: ProfilePanelProps) {
  const [form, setForm] = useState<ProfileFormState>(emptyProfileForm)
  const [deleteTarget, setDeleteTarget] = useState<OpnsenseProfile | null>(null)
  const [error, setError] = useState("")
  const [testingId, setTestingId] = useState<number | null>(null)
  const [testResult, setTestResult] = useState<string | null>(null)

  async function save(): Promise<void> {
    setError("")
    setTestResult(null)
    if ((form.apiKey === "") !== (form.apiSecret === "")) {
      setError("Enter both the API key and secret, or leave both blank to keep stored credentials.")
      return
    }
    const request = {
      name: form.name,
      host: form.host,
      port: form.port,
      allowInvalidCerts: form.allowInvalidCerts,
      sshPort: form.sshPort,
    }
    try {
      const saved =
        form.id === null ? await onCreate(request) : await onUpdate({ ...request, id: form.id })
      if (saved === null) return
      // Blank credentials on edit retain the stored entry; non-empty ones
      // overwrite it as a single keyring write.
      if (form.apiKey !== "" || form.apiSecret !== "") {
        if (await onSetCredentials(saved.id, form.apiKey, form.apiSecret)) {
          setForm(emptyProfileForm)
        }
      } else {
        setForm(emptyProfileForm)
      }
    } catch (err) {
      setError(opnsenseErrorPayload(err).message)
    }
  }

  async function testConnection(profile: OpnsenseProfile): Promise<void> {
    setTestingId(profile.id)
    setError("")
    setTestResult(null)
    try {
      const result = await onTestConnection(profile.id)
      if (result !== null) {
        setTestResult(
          result.connected
            ? result.authorized
              ? `${profile.name}: reachable, API key authorized`
              : `${profile.name}: reachable but the API key is NOT authorized — check key/secret and privileges`
            : `${profile.name}: unreachable`,
        )
      }
    } catch (err) {
      setError(opnsenseErrorPayload(err).message)
    } finally {
      setTestingId(null)
    }
  }

  return (
    <div className={styles.tabStack} data-testid="opnsense-profile-panel">
      <div className={styles.profileList}>
        {profiles.map((profile) => (
          <article key={profile.id} className={styles.profileRow} data-testid={`opnsense-profile-${profile.id}`}>
            <div>
              <h3>{profile.name}</h3>
              <p className={styles.profileMeta}>
                https://{profile.host}:{profile.port} · ssh:{profile.sshPort}
                {profile.allowInvalidCerts ? " · self-signed allowed" : ""} ·{" "}
                {profile.hasCredentials ? "API credentials stored" : "no API credentials"} ·{" "}
                {profile.sshHostKeyVerified ? "SSH host key verified" : "SSH host key unknown"}
              </p>
            </div>
            <div className={styles.rowActions}>
              <Button small variant="secondary" onClick={() => setForm({
                id: profile.id,
                name: profile.name,
                host: profile.host,
                port: profile.port,
                allowInvalidCerts: profile.allowInvalidCerts,
                sshPort: profile.sshPort,
                apiKey: "",
                apiSecret: "",
              })}>
                Edit
              </Button>
              <Button small variant="secondary" disabled={testingId === profile.id} onClick={() => void testConnection(profile)}>
                Test connection
              </Button>
              <Button small variant="outline-danger" onClick={() => setDeleteTarget(profile)}>
                Delete
              </Button>
            </div>
          </article>
        ))}
        {profiles.length === 0 ? <p className={styles.hint}>No OPNsense profiles yet — create one below.</p> : null}
      </div>

      <form className={styles.formGrid} onSubmit={(event) => { event.preventDefault(); void save() }}>
        <h2>{form.id === null ? "Create profile" : "Edit profile"}</h2>
        <label>
          Profile name
          <input value={form.name} onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, name: value })) }} />
        </label>
        <label>
          Host (DNS name or IP, no scheme)
          <input value={form.host} autoComplete="off" onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, host: value })) }} />
        </label>
        <label>
          HTTPS port
          <input type="number" min="1" max="65535" value={form.port} onChange={(event) => { const value = event.currentTarget.valueAsNumber; setForm((current) => ({ ...current, port: value })) }} />
        </label>
        <label>
          SSH port
          <input type="number" min="1" max="65535" value={form.sshPort} onChange={(event) => { const value = event.currentTarget.valueAsNumber; setForm((current) => ({ ...current, sshPort: value })) }} />
        </label>
        <label className={styles.checkboxRow}>
          <input type="checkbox" checked={form.allowInvalidCerts} onChange={(event) => { const checked = event.currentTarget.checked; setForm((current) => ({ ...current, allowInvalidCerts: checked })) }} />
          Accept self-signed certificates
        </label>
        <label>
          API key (read-only)
          <input type="password" value={form.apiKey} autoComplete="new-password" onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, apiKey: value })) }} />
        </label>
        <label>
          API secret
          <input type="password" value={form.apiSecret} autoComplete="new-password" onChange={(event) => { const value = event.currentTarget.value; setForm((current) => ({ ...current, apiSecret: value })) }} />
        </label>
        {form.id !== null ? <p className={styles.hint}>Leave API key/secret blank to keep the stored credentials. Changing the host or SSH port clears the pinned SSH host key.</p> : null}
        <div className={styles.formActions}>
          <Button type="button" onClick={() => setForm(emptyProfileForm)}>New</Button>
          <Button type="submit" variant="primary">Save profile</Button>
        </div>
      </form>

      {testResult !== null ? <p className={styles.success} data-testid="opnsense-test-result">{testResult}</p> : null}
      {error !== "" ? <p className={styles.error} role="alert">{error}</p> : null}
      {deleteTarget !== null ? (
        <ConfirmDialog
          message={`Delete profile ${deleteTarget.name}? Disconnect its monitor, log stream and terminal first if any are active.`}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget
            setDeleteTarget(null)
            void onDelete(target.id)
          }}
        />
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Overview tab (resources + PF state capacity)
// ---------------------------------------------------------------------------

function OverviewTab({ device }: { readonly device: OpnsenseDeviceState | null }) {
  const system = device?.system ?? null
  const placeholder = sectionPlaceholder(system, "Connect to collect firewall status.")
  const resources = system?.data?.resources ?? null
  const pfStates = system?.data?.pfStates ?? null
  const memPct =
    resources?.memUsedBytes != null && resources?.memTotalBytes != null && resources.memTotalBytes > 0
      ? (resources.memUsedBytes / resources.memTotalBytes) * 100
      : null
  const statePct =
    pfStates?.current != null && pfStates?.max != null && pfStates.max > 0
      ? (pfStates.current / pfStates.max) * 100
      : null

  return (
    <div className={styles.tabStack} data-testid="opnsense-overview">
      {placeholder}
      {sectionRefreshNotice(system)}
      {system !== null && system.data !== null ? (
        <div className={styles.cards}>
          <Card className={styles.card}>
            <h3 className={styles.cardTitle}>
              Resources {staleTag(system)}
            </h3>
            <div className={styles.statGrid}>
              <div>
                <div className={styles.statLabel}>CPU</div>
                <div className={styles.statValue}>
                  {resources?.cpuUsagePercent != null ? `${resources.cpuUsagePercent.toFixed(0)}%` : "—"}
                </div>
              </div>
              <div>
                <div className={styles.statLabel}>Memory</div>
                <div className={styles.statValue}>
                  {resources?.memUsedBytes != null ? formatBytes(resources.memUsedBytes) : "—"}
                </div>
                {resources?.memTotalBytes != null ? (
                  <div className={styles.statSub}>of {formatBytes(resources.memTotalBytes)}</div>
                ) : null}
              </div>
              <div>
                <div className={styles.statLabel}>Uptime</div>
                <div className={styles.statValue}>{resources?.uptime ?? "—"}</div>
              </div>
            </div>
            {system.data.resourcesError != null ? (
              <p className={styles.error}>Resources unavailable: {system.data.resourcesError.message}</p>
            ) : null}
          </Card>

          <Card className={styles.card}>
            <h3 className={styles.cardTitle}>
              PF state table {staleTag(system)}
            </h3>
            <div className={styles.statGrid}>
              <div>
                <div className={styles.statLabel}>Current states</div>
                <div className={styles.statValue}>{dash(pfStates?.current)}</div>
              </div>
              <div>
                <div className={styles.statLabel}>Capacity</div>
                <div className={styles.statValue}>{dash(pfStates?.max)}</div>
                {statePct !== null ? (
                  <div className={styles.statSub}>{statePct.toFixed(1)}% in use</div>
                ) : null}
              </div>
            </div>
            {system.data.pfStatesError != null ? (
              <p className={styles.error}>PF state stats unavailable: {system.data.pfStatesError.message}</p>
            ) : null}
          </Card>
        </div>
      ) : null}
      {memPct !== null && memPct >= 90 ? (
        <p className={`${styles.banner} ${styles.bannerWarn}`}>Memory usage is above 90% — the firewall may start rejecting new states.</p>
      ) : null}
      {statePct !== null && statePct >= 80 ? (
        <p className={`${styles.banner} ${styles.bannerWarn}`}>PF state table is above 80% of capacity — consider raising the limit or trimming keep states.</p>
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Firewall rules tab
// ---------------------------------------------------------------------------

type InterfaceDisplay = {
  readonly label: string
  readonly order: number
}

type RuleGroup = {
  readonly key: string
  readonly label: string
  readonly order: number
  readonly rules: readonly OpnsenseRuleDto[]
}

const EMPTY_INTERFACES: readonly OpnsenseInterfaceDto[] = []

function interfaceDisplays(interfaces: readonly OpnsenseInterfaceDto[]): ReadonlyMap<string, InterfaceDisplay> {
  const displays = new Map<string, InterfaceDisplay>()
  interfaces.forEach((iface, order) => {
    const identifier = iface.identifier?.trim()
    if (!identifier) return
    displays.set(identifier.toLowerCase(), {
      label: iface.description?.trim() || iface.name.trim() || identifier,
      order,
    })
  })
  return displays
}

function ruleInterfaceLabel(interfaceList: string, displays: ReadonlyMap<string, InterfaceDisplay>): string {
  const trimmed = interfaceList.trim()
  if (trimmed === "") return "Floating / all interfaces"
  return trimmed.split(",").map((token) => {
    const normalized = token.trim()
    return displays.get(normalized.toLowerCase())?.label ?? normalized
  }).join(", ")
}

/** Rules remain grouped by their complete interface list. Known interfaces
    follow firewall order; unknown tokens and Floating rules sort last. */
function groupRules(rules: readonly OpnsenseRuleDto[], displays: ReadonlyMap<string, InterfaceDisplay>): RuleGroup[] {
  const groups = new Map<string, { label: string; order: number; rules: OpnsenseRuleDto[] }>()
  for (const rule of rules) {
    const trimmed = rule.interface.trim()
    const tokens = trimmed === "" ? [] : trimmed.split(",").map((token) => token.trim().toLowerCase())
    const key = tokens.join(",")
    let group = groups.get(key)
    if (group === undefined) {
      const order = tokens.reduce(
        (minimum, token) => Math.min(minimum, displays.get(token)?.order ?? Number.MAX_SAFE_INTEGER),
        Number.MAX_SAFE_INTEGER,
      )
      group = { label: ruleInterfaceLabel(trimmed, displays), order, rules: [] }
      groups.set(key, group)
    }
    group.rules.push(rule)
  }
  return [...groups.entries()]
    .map(([key, group]) => ({ key, ...group }))
    .sort((a, b) => {
      if (a.key === "") return 1
      if (b.key === "") return -1
      return a.order - b.order || a.label.localeCompare(b.label)
    })
}

function RuleGroupTable({ group }: { readonly group: RuleGroup }) {
  return (
    <HorizontalScrollArea label={`Firewall rules — ${group.label}`} className={styles.scroller}>
      <table className={styles.table} aria-label={`Firewall rules — ${group.label}`}>
        <thead>
          <tr>
            <th scope="col">Action</th>
            <th scope="col">Interface</th>
            <th scope="col">Description</th>
            <th scope="col">Source</th>
            <th scope="col">Destination</th>
            <th scope="col">Proto</th>
            <th scope="col" className={styles.num}>Hits (packets)</th>
            <th scope="col" className={styles.num}>Evaluations</th>
            <th scope="col" className={styles.num}>Bytes</th>
            <th scope="col" className={styles.num}>States</th>
          </tr>
        </thead>
        <tbody>
          {group.rules.map((rule) => (
            <tr key={rule.uuid} className={rule.enabled ? undefined : styles.rowDisabled} data-testid="opnsense-rule-row">
              <td><span className={`${styles.badge} ${actionBadgeClass(rule.action)}`}>{rule.action}</span></td>
              <td>{group.label}</td>
              <td>{rule.description || "—"}</td>
              <td className={styles.mono}>{rule.source}</td>
              <td className={styles.mono}>{rule.destination}</td>
              <td className={styles.mono}>{rule.protocol || "—"}</td>
              <td className={styles.num}>{dash(rule.packets)}</td>
              <td className={styles.num}>{dash(rule.evaluations)}</td>
              <td className={styles.num}>{formatBytes(rule.bytes)}</td>
              <td className={styles.num}>{dash(rule.states)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </HorizontalScrollArea>
  )
}

function RulesTab({ device }: { readonly device: OpnsenseDeviceState | null }) {
  const view = device?.rules ?? null
  const placeholder = sectionPlaceholder(view, "Connect to collect firewall rules.")
  const data = view?.data ?? null
  const interfaces = device?.system?.data?.interfaces ?? EMPTY_INTERFACES
  const displays = useMemo(() => interfaceDisplays(interfaces), [interfaces])
  const [query, setQuery] = useState("")
  const normalizedQuery = query.trim().toLowerCase()
  const filteredRules = useMemo(() => {
    if (data === null) return []
    if (normalizedQuery === "") return data.rules
    return data.rules.filter((rule) => [
      rule.action,
      rule.interface,
      ruleInterfaceLabel(rule.interface, displays),
      rule.description,
      rule.source,
      rule.destination,
      rule.protocol,
    ].some((value) => value.toLowerCase().includes(normalizedQuery)))
  }, [data, displays, normalizedQuery])
  const groups = useMemo(() => groupRules(filteredRules, displays), [displays, filteredRules])
  const [collapsedKeys, setCollapsedKeys] = useState<ReadonlySet<string>>(new Set())

  const toggleGroup = useCallback((key: string) => {
    setCollapsedKeys((current) => {
      const next = new Set(current)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }, [])

  return (
    <div className={styles.tabStack} data-testid="opnsense-rules">
      {placeholder}
      {sectionRefreshNotice(view)}
      {data !== null ? (
        <>
          {data.truncated ? (
            <p className={`${styles.banner} ${styles.bannerWarn}`} role="alert">
              Rule list incomplete: showing {data.rules.length} of {data.total} reported rules.
              OPNsense may ignore GET pagination; a read-only key cannot use POST to fetch more.
            </p>
          ) : null}
          {data.countersError != null ? (
            <p className={styles.banner} role="alert">
              Hit counters unavailable: {data.countersError.message} — rule definitions are shown without counters.
            </p>
          ) : null}
          <div className={styles.toolbarRow}>
            {groups.length > 0 ? (
              <>
                <Button variant="secondary" onClick={() => setCollapsedKeys(new Set(groups.map((group) => group.key)))}>
                  Collapse all
                </Button>
                <Button variant="secondary" onClick={() => setCollapsedKeys(new Set())}>
                  Expand all
                </Button>
              </>
            ) : null}
            <input
              type="search"
              className={styles.search}
              value={query}
              onChange={(event) => setQuery(event.currentTarget.value)}
              placeholder="Search all rules"
              aria-label="Filter firewall rules"
            />
          </div>
          {groups.map((group) => {
            const collapsed = collapsedKeys.has(group.key)
            let hitsSum: number | null = null
            for (const rule of group.rules) {
              if (rule.packets != null) hitsSum = (hitsSum ?? 0) + rule.packets
            }
            return (
              <section key={group.key} className={styles.ruleGroup} data-testid="opnsense-rule-group-section">
                <button
                  type="button"
                  className={styles.ruleGroupHeader}
                  aria-expanded={!collapsed}
                  data-testid="opnsense-rule-group"
                  onClick={() => toggleGroup(group.key)}
                >
                  <span className={styles.ruleGroupChevron} aria-hidden="true">{collapsed ? "▸" : "▾"}</span>
                  <span className={styles.ruleGroupLabel}>{group.label}</span>
                  <span className={styles.hint}>
                    {group.rules.length === 1 ? "1 rule" : `${group.rules.length} rules`}
                  </span>
                  {hitsSum !== null ? (
                    <span className={styles.hint}>Σ {hitsSum.toLocaleString()} hits</span>
                  ) : null}
                </button>
                {collapsed ? null : <RuleGroupTable group={group} />}
              </section>
            )
          })}
          {data.rules.length > 0 && filteredRules.length === 0 ? (
            <p className={styles.empty}>No rules match the current search.</p>
          ) : null}
          {normalizedQuery !== "" && filteredRules.length > 0 ? (
            <p className={styles.hint}>
              {filteredRules.length} of {data.rules.length} loaded rules match the search.
            </p>
          ) : null}
          {data.rules.length === 0 ? <p className={styles.empty}>No rules reported.</p> : null}
          <p className={styles.hint}>
            {data.rules.length === data.total
              ? `${data.total} rules. `
              : `${data.rules.length} of ${data.total} reported rules. `}
            Rules reported by OPNsense; older versions may omit legacy rules. PF counters reset on
            reload and are not persistent totals.
          </p>
          {data.unmappedRunning.length > 0 ? (
            <section data-testid="opnsense-unmapped-rules">
              <h3 className={styles.cardTitle}>Unmapped running rules</h3>
              <HorizontalScrollArea label="Unmapped running rules" className={styles.scroller}>
                <table className={styles.table} aria-label="Unmapped running rules">
                  <thead>
                    <tr>
                      <th scope="col">PF ID</th>
                      <th scope="col">Description</th>
                      <th scope="col" className={styles.num}>Hits (packets)</th>
                      <th scope="col" className={styles.num}>Bytes</th>
                      <th scope="col" className={styles.num}>States</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.unmappedRunning.map((entry) => (
                      <tr key={entry.id}>
                        <td className={styles.mono}>{entry.id}</td>
                        <td>{entry.description ?? "—"}</td>
                        <td className={styles.num}>{dash(entry.packets)}</td>
                        <td className={styles.num}>{dash(entry.bytes)}</td>
                        <td className={styles.num}>{dash(entry.states)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </HorizontalScrollArea>
              <p className={styles.hint}>
                Active PF rules/stats that match no reported rule UUID. Numeric PF IDs are never
                joined to rule UUIDs.
              </p>
            </section>
          ) : null}
        </>
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Firewall logs tab
// ---------------------------------------------------------------------------

const LOG_TAB_OPTIONS: readonly { value: OpnsenseLogTab; label: string }[] = [
  { value: "all", label: "All" },
  { value: "blocked", label: "Blocked" },
  { value: "allowed", label: "Allowed" },
]

function LogsTab({ profileId }: { readonly profileId: number | null }) {
  const logs = useOpnsenseLogs()
  const scrollerRef = useRef<HTMLDivElement | null>(null)

  // Save each profile's capped buffer and stop only the previous stream.
  // Layout timing prevents one firewall's rows from flashing under another
  // profile name before the selection update paints.
  useLayoutEffect(() => {
    logs.selectProfile(profileId)
  }, [profileId, logs.selectProfile])

  useEffect(() => {
    const node = scrollerRef.current
    if (node !== null) node.scrollTop = 0
  }, [logs.entries])

  return (
    <div className={styles.tabStack} data-testid="opnsense-logs">
      <p className={styles.hint}>
        Only traffic matched by firewall rules with logging enabled appears. Blocked counts
        block/reject decisions; any other action is shown in All only.
      </p>
      <div className={styles.toolbar}>
        <div className={styles.toolbarRow}>
          <Button variant="primary" disabled={logs.running || profileId === null} onClick={() => profileId !== null && void logs.start(profileId)}>
            Connect
          </Button>
          <Button variant="outline-accent" disabled={!logs.running} onClick={() => void logs.stop()}>
            Disconnect
          </Button>
          <Button variant="secondary" disabled={logs.entries.length === 0 && !logs.running} onClick={() => logs.clear()}>
            Clear
          </Button>
          {logs.running ? <Live /> : null}
        </div>
        <div className={styles.toolbarRow}>
          <Segmented options={LOG_TAB_OPTIONS} value={logs.tab} onChange={logs.setTab} ariaLabel="Log action filter" />
          <input
            type="search"
            className={styles.search}
            value={logs.textFilter}
            onChange={(event) => logs.setTextFilter(event.currentTarget.value)}
            placeholder="Filter addresses, interfaces, protocols"
            aria-label="Filter log entries"
          />
        </div>
      </div>

      {logs.error !== "" ? <p className={styles.banner} role="alert">{logs.error}</p> : null}

      <HorizontalScrollArea label="Firewall logs" className={styles.scroller} scrollRef={scrollerRef}>
        <table className={styles.table} aria-label="Firewall log entries">
          <thead>
            <tr>
              <th scope="col">Time</th>
              <th scope="col">Action</th>
              <th scope="col">Interface</th>
              <th scope="col">Dir</th>
              <th scope="col">Proto</th>
              <th scope="col">Source</th>
              <th scope="col">Destination</th>
              <th scope="col">Rule</th>
            </tr>
          </thead>
          <tbody>
            {logs.entries.map((entry, index) => (
              <tr key={`${entry.receivedAt}-${index}`} data-testid="opnsense-log-row" data-action={entry.action}>
                <td className={styles.mono}>{entry.timestamp ?? entry.receivedAt}</td>
                <td><span className={`${styles.badge} ${actionBadgeClass(entry.action)}`}>{entry.action}</span></td>
                <td>{entry.interfaceName ?? "—"}</td>
                <td className={styles.mono}>{entry.dir ?? "—"}</td>
                <td className={styles.mono}>{entry.proto ?? "—"}</td>
                <td className={styles.mono}>{entry.src ?? "—"}{entry.srcPort !== null && entry.srcPort !== undefined ? `:${entry.srcPort}` : ""}</td>
                <td className={styles.mono}>{entry.dst ?? "—"}{entry.dstPort !== null && entry.dstPort !== undefined ? `:${entry.dstPort}` : ""}</td>
                <td className={styles.mono}>{entry.label ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {logs.entries.length === 0 ? (
          <p className={styles.empty} data-testid="opnsense-logs-empty">
            {logs.running ? "Waiting for log entries…" : "Connect to the stream to see firewall logs."}
          </p>
        ) : null}
      </HorizontalScrollArea>
      <p className={styles.hint}>
        {logs.entries.length === logs.totalCount
          ? `${logs.totalCount} entries (newest 500 kept)`
          : `${logs.entries.length} of ${logs.totalCount} entries (filter active)`}
      </p>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Interfaces & VPN tab
// ---------------------------------------------------------------------------

function InterfaceTable({ interfaces }: { readonly interfaces: readonly OpnsenseInterfaceDto[] }) {
  if (interfaces.length === 0) return <p className={styles.hint}>No interfaces reported.</p>
  return (
    <HorizontalScrollArea label="Network interfaces" className={styles.scroller}>
      <table className={`${styles.table} ${styles.ifaceTable}`} aria-label="Network interfaces">
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Description</th>
            <th scope="col">Link</th>
            <th scope="col">MAC</th>
            <th scope="col" className={styles.num}>MTU</th>
            <th scope="col">Media</th>
            <th scope="col">IPv4</th>
            <th scope="col">IPv6</th>
            <th scope="col" className={styles.num}>RX bytes</th>
            <th scope="col" className={styles.num}>TX bytes</th>
            <th scope="col" className={styles.num}>RX packets</th>
            <th scope="col" className={styles.num}>TX packets</th>
          </tr>
        </thead>
        <tbody>
          {interfaces.map((item) => (
            <tr key={item.name} className={item.enabled ? undefined : styles.rowDisabled}>
              <td className={styles.mono}>
                {item.name}
                {item.lagg != null ? (
                  <div className={styles.laggLine}>
                    {item.name} · {item.lagg.proto ?? "lagg"} —{" "}
                    {item.lagg.members.map((member) => {
                      const details = [...member.state, member.media ?? null, member.link ?? null]
                        .filter((part): part is string => part !== null && part !== "")
                        .join(", ")
                      return `${member.name}${details !== "" ? ` (${details})` : ""}`
                    }).join(", ")}
                  </div>
                ) : null}
              </td>
              <td>{item.description ?? "—"}</td>
              <td>
                {item.link !== null && item.link !== undefined ? (
                  <span className={`${styles.badge} ${item.link === "up" ? styles.badgeOk : styles.badgeErr}`}>{item.link}</span>
                ) : (
                  "—"
                )}
              </td>
              <td className={styles.mono}>{item.macaddr ?? "—"}</td>
              <td className={styles.num}>{item.mtu ?? "—"}</td>
              <td className={styles.mono}>{item.media ?? "—"}</td>
              <td className={styles.mono}>{item.ipv4.length > 0 ? item.ipv4.join(", ") : "—"}</td>
              <td className={styles.mono}>{item.ipv6.length > 0 ? item.ipv6.join(", ") : "—"}</td>
              <td className={styles.num}>{formatBytes(item.rxBytes)}</td>
              <td className={styles.num}>{formatBytes(item.txBytes)}</td>
              <td className={styles.num}>{dash(item.rxPackets)}</td>
              <td className={styles.num}>{dash(item.txPackets)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </HorizontalScrollArea>
  )
}

function WireguardPeerStatus({ peer }: { readonly peer: OpnsenseWireguardPeerDto }) {
  const badge =
    peer.status === "active" ? styles.badgeOk
    : peer.status === "stale" ? styles.badgeWarn
    : styles.badgeMuted
  const detail =
    peer.status === "never" ? "never handshaked"
    : peer.handshakeAgeSecs != null ? formatHandshakeAge(peer.handshakeAgeSecs)
    : "—"
  return (
    <span title={peer.status === "stale" ? "Handshake is older than the WireGuard timeout — the tunnel is stale, not necessarily offline." : undefined}>
      <span className={`${styles.badge} ${badge}`}>{peer.status}</span>{" "}
      <span className={styles.hint}>{detail}</span>
    </span>
  )
}

function VpnServiceCard<T>({ title, service, children }: {
  readonly title: string
  readonly service: OpnsenseVpnServiceDto<T>
  readonly children: (data: T) => ReactNode
}) {
  return (
    <article className={styles.card} data-testid={`opnsense-vpn-${title.toLowerCase().replace(/[^a-z]+/g, "-")}`}>
      <h3 className={styles.cardTitle}>{title}</h3>
      {service.state === "ok" && service.data != null ? (
        children(service.data)
      ) : (
        <p className={service.state === "access-denied" ? styles.error : styles.hint}>
          {service.message ??
            (service.state === "access-denied"
              ? "Access denied — the API key lacks privileges for this service"
              : "Unavailable")}
        </p>
      )}
    </article>
  )
}

function VpnTab({ device }: { readonly device: OpnsenseDeviceState | null }) {
  const system = device?.system ?? null
  const vpn = device?.vpn ?? null
  const interfaces = system?.data?.interfaces ?? []
  const vpnData = vpn?.data ?? null

  return (
    <div className={styles.tabStack} data-testid="opnsense-vpn">
      {sectionPlaceholder(system, "Connect to collect interface status.")}
      {sectionRefreshNotice(system)}
      {system?.data?.interfacesTruncated ? (
        <p className={`${styles.banner} ${styles.bannerWarn}`} role="alert">
          Interface list incomplete — OPNsense may ignore GET pagination, and this read-only
          monitor will not use POST to fetch more.
        </p>
      ) : null}
      {interfaces.length > 0 ? <InterfaceTable interfaces={interfaces} /> : null}
      {sectionPlaceholder(vpn, "Connect to collect VPN status.")}
      {sectionRefreshNotice(vpn)}
      {vpnData !== null ? (
        <div className={styles.cards}>
          <VpnServiceCard title="WireGuard" service={vpnData.wireguard}>
            {(data) => (
              data.interfaces.length === 0 ? <p className={styles.hint}>No WireGuard interfaces configured.</p> : (
                <div className={styles.tabStack}>
                  {data.interfaces.map((wgInterface) => (
                    <div key={wgInterface.name}>
                      <div className={styles.statLabel}>{wgInterface.name}</div>
                      {wgInterface.peers.length === 0 ? (
                        <p className={styles.hint}>No peers.</p>
                      ) : (
                        <ul className={styles.upgradeList}>
                          {wgInterface.peers.map((peer) => (
                            <li key={peer.name}>
                              <span>{peer.name}</span> <WireguardPeerStatus peer={peer} />
                              {peer.endpoint != null ? <span className={styles.hint}> — {peer.endpoint}</span> : null}
                              {(peer.transferRxBytes != null || peer.transferTxBytes != null) ? (
                                <span className={styles.hint}>
                                  {" "}↓ {formatBytes(peer.transferRxBytes)} ↑ {formatBytes(peer.transferTxBytes)}
                                </span>
                              ) : null}
                            </li>
                          ))}
                        </ul>
                      )}
                    </div>
                  ))}
                </div>
              )
            )}
          </VpnServiceCard>

          <VpnServiceCard title="OpenVPN" service={vpnData.openvpn}>
            {(sessions) => (
              sessions.length === 0 ? <p className={styles.hint}>No sessions.</p> : (
                <ul className={styles.upgradeList}>
                  {sessions.map((session) => (
                    <li key={session.name}>
                      <span className={`${styles.badge} ${session.connected ? styles.badgeOk : styles.badgeMuted}`}>
                        {session.connected ? "connected" : "down"}
                      </span>{" "}
                      <span>{session.name}</span>
                      {session.description != null ? <span className={styles.hint}> — {session.description}</span> : null}
                      <span className={styles.hint}> ({session.kind})</span>
                      {session.realAddress != null ? <span className={styles.hint}> · from {session.realAddress}</span> : null}
                      {session.virtualAddress != null ? <span className={styles.hint}> · tunnel {session.virtualAddress}</span> : null}
                      {session.connectedSince != null ? <span className={styles.hint}> · since {session.connectedSince}</span> : null}
                    </li>
                  ))}
                </ul>
              )
            )}
          </VpnServiceCard>

          <VpnServiceCard title="IPsec" service={vpnData.ipsec}>
            {(sessions) => (
              sessions.length === 0 ? <p className={styles.hint}>No sessions.</p> : (
                <ul className={styles.upgradeList}>
                  {sessions.map((session) => (
                    <li key={session.name}>
                      <span className={`${styles.badge} ${session.connected ? styles.badgeOk : styles.badgeMuted}`}>
                        {session.connected ? "connected" : "down"}
                      </span>{" "}
                      <span>{session.name}</span>
                      {session.description != null ? <span className={styles.hint}> — {session.description}</span> : null}
                      {session.remoteHost != null ? <span className={styles.hint}> · {session.remoteHost}</span> : null}
                      {session.version != null ? <span className={styles.hint}> · {session.version}</span> : null}
                    </li>
                  ))}
                </ul>
              )
            )}
          </VpnServiceCard>

          <VpnServiceCard title="Tailscale" service={vpnData.tailscale}>
            {(data) => (
              <div className={styles.tabStack}>
                <div className={styles.statGrid}>
                  <div>
                    <div className={styles.statLabel}>Backend</div>
                    <div className={styles.statValue}>{data.backendState ?? "—"}</div>
                  </div>
                  <div>
                    <div className={styles.statLabel}>Service</div>
                    <div className={styles.statValue}>
                      {data.serviceRunning == null ? "—" : data.serviceRunning ? "running" : "stopped"}
                    </div>
                  </div>
                  <div>
                    <div className={styles.statLabel}>Hostname</div>
                    <div className={styles.statValue}>{data.hostname ?? "—"}</div>
                  </div>
                  <div>
                    <div className={styles.statLabel}>Own IPs</div>
                    <div className={styles.statValue}>{data.ownIps.length > 0 ? data.ownIps.join(", ") : "—"}</div>
                  </div>
                </div>
                {data.peers.length === 0 ? (
                  <p className={styles.hint}>No peers.</p>
                ) : (
                  <ul className={styles.upgradeList}>
                    {data.peers.map((peer) => (
                      <li key={peer.name} className={styles.peerRow}>
                        <span className={`${styles.badge} ${peer.online === true ? styles.badgeOk : peer.online === false ? styles.badgeMuted : styles.badgeWarn}`}>
                          {peer.online == null ? "unknown" : peer.online ? "online" : "offline"}
                        </span>
                        <span className={styles.peerBody}>
                          <span className={styles.peerName}>{peer.name}</span>
                          <span className={styles.hint}>{peer.ips.join(", ")}{peer.lastSeen != null ? ` · last seen ${peer.lastSeen}` : ""}</span>
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </VpnServiceCard>
        </div>
      ) : null}
      <p className={styles.hint}>Refreshes every 5 s while monitoring runs.</p>
    </div>
  )
}

function FirmwareTab({ profileId, firmware, onRefresh }: {
  readonly profileId: number | null
  readonly firmware: { readonly profileId: number; readonly check: OpnsenseVersionCheckDto } | null
  readonly onRefresh: (profileId?: number) => Promise<unknown>
}) {
  const [busy, setBusy] = useState(false)
  const check = firmware !== null && firmware.profileId === profileId ? firmware.check : null

  async function refresh(): Promise<void> {
    if (profileId === null) return
    setBusy(true)
    try {
      await onRefresh(profileId)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className={styles.tabStack} data-testid="opnsense-firmware">
      <p className={styles.hint}>
        Compares installed firmware with the changelog cached on the firewall. The firewall
        fetches it during its own update checks; this read-only app never triggers a check
        or installs updates. The cached core and plugin results may be stale.
      </p>
      <div className={styles.toolbarRow}>
        <Button variant="primary" disabled={profileId === null || busy} onClick={() => void refresh()}>
          {busy ? "Reading…" : "Read cached status"}
        </Button>
      </div>
      {check !== null ? (
        <div className={styles.tabStack}>
          {check.state === "update-available" ? (
            <p className={`${styles.banner} ${styles.bannerWarn}`} data-testid="opnsense-update-available">
              Firmware update available: {check.installedVersion ?? "unknown"} → {check.latestVersion}
            </p>
          ) : null}
          {check.state === "up-to-date" ? (
            <p className={styles.success}>No newer release in the firewall&apos;s cached {check.series} changelog.</p>
          ) : null}
          {check.state === "unknown" || check.state === "error" ? (
            <p className={`${styles.banner} ${styles.bannerWarn}`} role="status" data-testid="opnsense-firmware-unknown">
              Update availability unknown: {check.message ?? "No cached release changelog is available."}
            </p>
          ) : null}
          <p className={styles.hint} data-testid="opnsense-firmware-meta">
            Installed: {check.installedVersion ?? "unknown"} · Latest cached: {check.latestVersion ?? "unknown"} ·
            {" "}ABI: {check.abi ?? "unknown"} · Cache read: {check.checkedAt}
          </p>
          {check.changelog.length > 0 ? (
            <Card className={styles.card}>
              <h3 className={styles.cardTitle}>Newer releases</h3>
              <ul className={styles.upgradeList}>
                {check.changelog.map((entry) => (
                  <li key={entry.version}>{entry.version}{entry.date != null ? ` · ${entry.date}` : ""}</li>
                ))}
              </ul>
            </Card>
          ) : null}
          <Card className={styles.card}>
            <h3 className={styles.cardTitle}>Installed plugins</h3>
            {check.plugins.length === 0 ? <p className={styles.hint}>No installed plugins reported.</p> : (
              <ul className={styles.upgradeList}>
                {check.plugins.map((plugin) => (
                  <li key={plugin.name}>
                    {plugin.name} · {plugin.version ?? "version unknown"}
                    {plugin.cachedUpgradeVersion != null ? (
                      <span className={`${styles.badge} ${styles.badgeWarn}`}>
                        cached upgrade → {plugin.cachedUpgradeVersion}
                      </span>
                    ) : <span className={styles.hint}>update availability unknown</span>}
                  </li>
                ))}
              </ul>
            )}
            <p className={styles.hint}>
              Check System ▸ Firmware ▸ Plugins in OPNsense for current plugin updates.
            </p>
          </Card>
        </div>
      ) : (
        <p className={styles.hint}>Select a profile to read its cached release changelog.</p>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Backups tab (saved revisions, encrypted locally)
// ---------------------------------------------------------------------------

const BACKUP_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/
const RESERVED_NAME = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i

function defaultBackupName(now = new Date()): string {
  const two = (value: number) => String(value).padStart(2, "0")
  return `opnsense-config-${now.getFullYear()}${two(now.getMonth() + 1)}${two(now.getDate())}-${two(now.getHours())}${two(now.getMinutes())}${two(now.getSeconds())}`
}

function validBackupName(name: string): boolean {
  return BACKUP_NAME.test(name) && !RESERVED_NAME.test(name)
}

function BackupsTab({ profileId, revisions, onRefresh, onBackup }: {
  readonly profileId: number | null
  readonly revisions: { readonly profileId: number; readonly items: readonly OpnsenseRevisionDto[] } | null
  readonly onRefresh: (profileId?: number) => Promise<unknown>
  readonly onBackup: (request: RunOpnsenseBackupRequest) => Promise<OpnsenseBackupResultDto | null>
}) {
  const [backupName, setBackupName] = useState(defaultBackupName)
  const [passphrase, setPassphrase] = useState("")
  const [destination, setDestination] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [result, setResult] = useState<OpnsenseBackupResultDto | null>(null)
  const items = revisions !== null && revisions.profileId === profileId ? revisions.items : null
  const [revisionId, setRevisionId] = useState<string | null>(null)
  const nameIsValid = validBackupName(backupName)
  const strength = passphraseStrength(passphrase)
  const canSubmit = profileId !== null && items !== null && items.length > 0 &&
    destination !== "" && strength === "ok" && nameIsValid && !busy
  const profileRef = useRef(profileId)
  profileRef.current = profileId

  // Load the revision list whenever the selected profile changes.
  const refreshRef = useRef(onRefresh)
  refreshRef.current = onRefresh
  useEffect(() => {
    setResult(null)
    setError("")
    setPassphrase("")
    setRevisionId(null)
    if (profileId !== null) void refreshRef.current(profileId)
  }, [profileId])

  async function chooseDirectory(): Promise<void> {
    const selected = await open({ directory: true, defaultPath: destination || undefined })
    if (typeof selected === "string") setDestination(selected)
  }

  async function run(): Promise<void> {
    if (!canSubmit || profileId === null) return
    setBusy(true)
    setError("")
    setResult(null)
    try {
      const next = await onBackup({ profileId, destinationDir: destination, backupName, passphrase, revisionId })
      if (profileRef.current !== profileId) return
      if (next !== null) {
        setResult(next)
      } else {
        setError("Backup failed — see the message above for details.")
      }
    } catch (err) {
      if (profileRef.current === profileId) setError(opnsenseErrorPayload(err).message)
    } finally {
      // Never retain a passphrase in the mounted view after an attempt.
      setPassphrase("")
      setBusy(false)
    }
  }

  return (
    <div className={styles.tabStack} data-testid="opnsense-backups">
      <section>
        <h3 className={styles.cardTitle}>Saved configuration revisions</h3>
        {items === null ? <p className={styles.hint}>Select a profile to list its saved revisions.</p> : null}
        {items !== null && items.length === 0 ? (
          <p className={styles.hint} data-testid="opnsense-no-revisions">No saved configuration revision.</p>
        ) : null}
        {items !== null && items.length > 0 ? (
          <HorizontalScrollArea label="Saved configuration revisions" className={styles.scroller}>
            <table className={styles.table} aria-label="Saved configuration revisions">
              <thead>
                <tr>
                  <th scope="col">Revision</th>
                  <th scope="col">Saved at</th>
                  <th scope="col">Description</th>
                  <th scope="col" className={styles.num}>Size</th>
                </tr>
              </thead>
              <tbody>
                {items.map((revision) => (
                  <tr key={revision.id}>
                    <td className={styles.mono}>{revision.id}</td>
                    <td className={styles.mono}>{revision.timeIso ?? "—"}</td>
                    <td>{revision.description ?? "—"}</td>
                    <td className={styles.num}>{formatBytes(revision.filesize)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </HorizontalScrollArea>
        ) : null}
        <p className={styles.hint}>
          The firewall keeps saved revisions on its own schedule — the newest one may be older than
          unsaved changes on the device.
        </p>
      </section>

      <form className={styles.formGrid} onSubmit={(event) => { event.preventDefault(); void run() }}>
        <h2>Encrypted backup</h2>
        <label>
          Backup name
          <input value={backupName} aria-invalid={!nameIsValid} onChange={(event) => setBackupName(event.currentTarget.value)} />
        </label>
        <label>
          Passphrase (required, never stored)
          <input type="password" value={passphrase} autoComplete="new-password" onChange={(event) => setPassphrase(event.currentTarget.value)} />
        </label>
        <p className={strength === "weak" ? styles.error : styles.hint}>{PASSPHRASE_HINT}</p>
        <label>
          Saved revision
          <select value={revisionId ?? ""} disabled={items === null || items.length === 0}
            onChange={(event) => setRevisionId(event.currentTarget.value || null)}>
            <option value="">Latest saved revision</option>
            {items?.map((revision) => (
              <option key={revision.id} value={revision.id}>
                {revision.id}{revision.timeIso != null ? ` · ${revision.timeIso}` : ""}
              </option>
            ))}
          </select>
        </label>
        {!nameIsValid ? <p className={styles.error}>Use 1-64 letters, numbers, dot, underscore, or dash; no Windows device names.</p> : null}
        <div className={styles.formActions}>
          <Button type="button" onClick={() => void chooseDirectory()}>Choose directory</Button>
          <span className={styles.hint}>{destination || "No directory selected"}</span>
        </div>
        {profileId === null ? <p className={styles.hint}>Select a profile above to create a backup.</p> : null}
        <div className={styles.formActions}>
          <Button type="submit" variant="primary" disabled={!canSubmit}>
            {busy ? "Saving…" : "Save encrypted backup"}
          </Button>
        </div>
      </form>

      {error !== "" ? <p className={styles.error} role="alert">{error}</p> : null}
      {result !== null ? (
        <div className={styles.note} data-testid="opnsense-backup-result">
          <p>
            Saved <code>{result.path}</code>
          </p>
          <p>
            Source: <strong>saved</strong> firewall revision <code>{result.sourceRevisionId}</code>
            {result.sourceTimeIso != null ? ` (${result.sourceTimeIso})` : ""} — possibly older than
            unsaved changes on the device.
          </p>
          <p>
            To restore, decrypt outside the app: <code>{"age -d -o config.xml <file>.xml.age"}</code>,
            then import the XML in the OPNsense GUI (System ▸ Configuration ▸ Backups). The app never
            stores plaintext XML and never restores by itself.
          </p>
        </div>
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Terminal tab (separate SSH login + host-key verification)
// ---------------------------------------------------------------------------

function OpnsenseTerminalSection({ profile, onActivated, onDeactivated, onProfilesChanged }: {
  readonly profile: OpnsenseProfile
  readonly onActivated: (profileId: number) => void
  readonly onDeactivated: (profileId: number) => void
  readonly onProfilesChanged: () => Promise<void>
}) {
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [notice, setNotice] = useState("")
  const [forgetTarget, setForgetTarget] = useState(false)
  const [forgetBusy, setForgetBusy] = useState(false)
  const usernameRef = useRef(username)
  const passwordRef = useRef(password)
  usernameRef.current = username
  passwordRef.current = password

  const adapter = useMemo<SshTerminalAdapter>(
    () => ({
      open: (cols, rows, onData, extra) =>
        opnsenseTerminalOpen(
          profile.id,
          usernameRef.current,
          passwordRef.current,
          cols,
          rows,
          typeof extra === "string" ? extra : null,
          onData,
        ).then((opened) => opened.terminalId),
      write: (terminalId, data) => opnsenseTerminalWrite(String(terminalId), data),
      resize: (terminalId, cols, rows) => opnsenseTerminalResize(String(terminalId), cols, rows),
      close: (terminalId) => opnsenseTerminalClose(String(terminalId)),
    }),
    [profile.id],
  )

  const handleActivated = useCallback(
    (profileId: number) => {
      // The password is never stored — clear it as soon as the shell opens.
      setPassword("")
      setNotice("")
      onActivated(profileId)
    },
    [onActivated],
  )

  async function forgetHostKey(): Promise<void> {
    setForgetTarget(false)
    setForgetBusy(true)
    try {
      await opnsenseForgetSshHostKey(profile.id)
      await onProfilesChanged()
      setNotice("Pinned host key forgotten — connect again to verify the new key.")
    } catch (err) {
      setNotice(opnsenseErrorPayload(err).message)
    } finally {
      setForgetBusy(false)
    }
  }

  return (
    <>
      <SshTerminalPanel
        profileId={profile.id}
        profileName={profile.name}
        profileHost={`${profile.host}:${profile.sshPort}`}
        adapter={adapter}
        testIdPrefix="opnsense-terminal"
        onActivated={handleActivated}
        onDeactivated={onDeactivated}
        renderIdle={({ openError, connect }) => {
          if (openError?.kind === "unknown-host-key") {
            return (
              <div className={styles.credentialBox} data-testid="opnsense-host-key-prompt">
                <p className={styles.hint}>
                  Unverified SSH host key. Confirm the SHA-256 fingerprint on the firewall before
                  accepting (firewall console or System ▸ Access ▸ Servers).
                </p>
                <p className={styles.fingerprint}>SHA-256:{openError.fingerprint ?? "?"}</p>
                <div className={styles.rowActions}>
                  <Button
                    variant="primary"
                    onClick={() => connect(openError.fingerprint)}
                  >
                    Verify fingerprint &amp; connect
                  </Button>
                </div>
              </div>
            )
          }
          if (openError?.kind === "host-key-mismatch") {
            return (
              <div className={styles.credentialBox} data-testid="opnsense-host-key-mismatch">
                <p className={styles.error}>{openError.message}</p>
                <div className={styles.rowActions}>
                  <Button variant="outline-danger" disabled={forgetBusy} onClick={() => setForgetTarget(true)}>
                    Forget pinned host key
                  </Button>
                </div>
              </div>
            )
          }
          return (
            <div className={styles.credentialBox} data-testid="opnsense-terminal-credentials">
              <p className={styles.hint}>
                Separate OPNsense SSH login — the read-only API key is never used here, and SSH
                credentials never call the HTTP API. SSH must be enabled on port {profile.sshPort}.
                Commands run only inside the authenticated session; the password is not stored.
              </p>
              {openError?.kind === "ssh-auth-failed" ? (
                <p className={styles.error}>SSH authentication failed for that user — check the username and password.</p>
              ) : null}
              <label>
                SSH username
                <input value={username} autoComplete="off" onChange={(event) => setUsername(event.currentTarget.value)} />
              </label>
              <label>
                SSH password
                <input type="password" value={password} autoComplete="new-password" onChange={(event) => setPassword(event.currentTarget.value)} />
              </label>
              {notice !== "" ? <p className={styles.success}>{notice}</p> : null}
            </div>
          )
        }}
      />
      {forgetTarget ? (
        <ConfirmDialog
          message={`Forget the pinned SSH host key for ${profile.name}? Only do this after an intentional rekey on the firewall.`}
          confirmLabel="Forget"
          onCancel={() => setForgetTarget(false)}
          onConfirm={() => void forgetHostKey()}
        />
      ) : null}
    </>
  )
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

export default function OpnsenseView() {
  const opnsense = useOpnsense()
  const [activeTab, setActiveTab] = useState<TabId>("profiles")
  /** Profiles with an activated terminal. Each gets a persistent panel
      instance (keyed by profile id) that stays mounted — hidden, not
      unmounted — while another profile is selected, so switching never
      drops a shell. */
  const [terminalProfiles, setTerminalProfiles] = useState<readonly number[]>([])
  /** Session-scoped silencer for the terminal-switch warning — never
      persisted, so a restart brings the warnings back. */
  const terminalWarningSilencedRef = useRef(false)
  const [terminalSwitchWarning, setTerminalSwitchWarning] = useState<OpnsenseProfile | null>(null)
  const selectedDevice = opnsense.selectedDevice
  const detachedProfileId =
    selectedDevice !== null && selectedDevice.running && !selectedDevice.attached
      ? selectedDevice.profileId
      : null

  // Prune terminals for deleted profiles — unmounting the panel closes the
  // backend session via the panel's unmount effect.
  useEffect(() => {
    setTerminalProfiles((current) =>
      current.filter((id) => opnsense.profiles.some((profile) => profile.id === id)),
    )
  }, [opnsense.profiles])

  const refreshProfiles = opnsense.refreshProfiles
  const activateTerminal = useCallback((profileId: number) => {
    setTerminalProfiles((current) => (current.includes(profileId) ? current : [...current, profileId]))
    void refreshProfiles()
  }, [refreshProfiles])
  const deactivateTerminal = useCallback((profileId: number) => {
    setTerminalProfiles((current) => current.filter((id) => id !== profileId))
  }, [])

  // Warn when the visible terminal switches to a different profile that has
  // an open terminal — the moment a wrong-firewall mistake becomes possible.
  const previousTerminalProfileRef = useRef<number | null>(null)
  useEffect(() => {
    const current = opnsense.selectedProfile?.id ?? null
    const previous = previousTerminalProfileRef.current
    previousTerminalProfileRef.current = current
    if (
      activeTab !== "terminal" ||
      current === null ||
      previous === null ||
      previous === current ||
      terminalWarningSilencedRef.current ||
      !terminalProfiles.includes(current)
    ) {
      return
    }
    const profile = opnsense.profiles.find((item) => item.id === current)
    if (profile !== undefined) setTerminalSwitchWarning(profile)
  }, [opnsense.selectedProfile?.id, activeTab, terminalProfiles, opnsense.profiles])

  // Terminal slots: every activated profile keeps a mounted panel; the
  // selected profile always gets one (ephemeral until it connects).
  const terminalSlotIds = new Set<number>(terminalProfiles)
  if (opnsense.selectedProfile !== null) terminalSlotIds.add(opnsense.selectedProfile.id)

  return (
    <section className={styles.view} data-testid="opnsense-view">
      <ViewHeader title={<h1>OPNsense</h1>} subtitle={opnsense.selectedProfile?.name ?? "no profile selected"}>
        <div className={`${styles.controls} vk-view-actions`}>
          <div className={styles.field}>
            <label htmlFor="opnsense-profile">Profile</label>
            <select
              id="opnsense-profile"
              value={opnsense.selectedProfile?.id ?? ""}
              onChange={(event) => {
                const profile =
                  opnsense.profiles.find((item) => String(item.id) === event.currentTarget.value) ?? null
                opnsense.selectProfile(profile)
              }}
            >
              <option value="">Select a profile</option>
              {opnsense.profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>{profile.name}</option>
              ))}
            </select>
          </div>
          <div className={styles.actions}>
            <Button
              variant="primary"
              disabled={opnsense.selectedProfile === null || (selectedDevice?.running ?? false)}
              onClick={() => opnsense.selectedProfile !== null && void opnsense.start(opnsense.selectedProfile.id)}
            >
              Connect
            </Button>
            <Button
              variant="outline-accent"
              disabled={!selectedDevice?.running || selectedDevice?.attached === false}
              onClick={() => void opnsense.stop()}
            >
              Disconnect
            </Button>
          </div>
        </div>
      </ViewHeader>

      {opnsense.error !== "" ? <div className={styles.banner}>{opnsense.error}</div> : null}

      {opnsense.activeDevices.length > 0 ? (
        <div className={styles.deviceStrip} role="list" aria-label="Active firewalls" data-testid="opnsense-device-strip">
          {opnsense.activeDevices.map((device) => {
            const profile = opnsense.profiles.find((item) => item.id === device.profileId)
            const name = profile?.name ?? `profile ${device.profileId}`
            const selected = device.profileId === opnsense.selectedProfile?.id
            return (
              <div
                key={device.profileId}
                role="listitem"
                className={`${styles.deviceChip}${selected ? ` ${styles.deviceChipSelected}` : ""}`}
                data-testid={`opnsense-device-${device.profileId}`}
              >
                <button
                  type="button"
                  className={styles.deviceSelect}
                  aria-pressed={selected}
                  onClick={() => profile !== undefined && opnsense.selectProfile(profile)}
                >
                  <Live />
                  <span>{name}</span>
                  {terminalProfiles.includes(device.profileId) ? (
                    <span
                      className={styles.deviceChipTerm}
                      style={{ "--terminal-accent": terminalAccentFor(device.profileId) } as CSSProperties}
                      title="An SSH terminal is open for this firewall"
                    >
                      term
                    </span>
                  ) : null}
                  {!device.attached ? (
                    <span className={styles.deviceDetached} title="Live, but this view is not receiving its data (the view was reloaded). Reconnect to resume.">detached</span>
                  ) : null}
                </button>
                <Button
                  variant="outline-danger"
                  disabled={!device.attached}
                  onClick={() => void opnsense.stop(device.profileId)}
                >
                  Disconnect
                </Button>
              </div>
            )
          })}
        </div>
      ) : null}

      {detachedProfileId !== null ? (
        <div className={styles.banner}>
          This monitor is live but this view is not receiving its data (the view was reloaded).{" "}
          <Button variant="outline-accent" onClick={() => void opnsense.reconnect(detachedProfileId)}>Reconnect</Button>
        </div>
      ) : null}

      <div className={styles.tabBar} role="tablist" aria-label="OPNsense sections">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            id={`opnsense-tab-${tab.id}`}
            type="button"
            role="tab"
            aria-controls={`opnsense-panel-${tab.id}`}
            aria-selected={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      <section className={styles.tabPanel} id="opnsense-panel-profiles" role="tabpanel" aria-labelledby="opnsense-tab-profiles" hidden={activeTab !== "profiles"}>
        <ProfilePanel
          profiles={opnsense.profiles}
          onCreate={opnsense.createProfile}
          onUpdate={opnsense.updateProfile}
          onDelete={async (id) => (await opnsense.deleteProfile(id)) !== null}
          onSetCredentials={opnsense.setCredentials}
          onTestConnection={opnsense.testConnection}
        />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-overview" role="tabpanel" aria-labelledby="opnsense-tab-overview" hidden={activeTab !== "overview"}>
        <OverviewTab device={selectedDevice} />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-rules" role="tabpanel" aria-labelledby="opnsense-tab-rules" hidden={activeTab !== "rules"}>
        <RulesTab device={selectedDevice} />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-logs" role="tabpanel" aria-labelledby="opnsense-tab-logs" hidden={activeTab !== "logs"}>
        <LogsTab profileId={opnsense.selectedProfile?.id ?? null} />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-vpn" role="tabpanel" aria-labelledby="opnsense-tab-vpn" hidden={activeTab !== "vpn"}>
        <VpnTab device={selectedDevice} />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-firmware" role="tabpanel" aria-labelledby="opnsense-tab-firmware" hidden={activeTab !== "firmware"}>
        <FirmwareTab
          profileId={opnsense.selectedProfile?.id ?? null}
          firmware={opnsense.firmware}
          onRefresh={opnsense.checkFirmwareUpdates}
        />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-backups" role="tabpanel" aria-labelledby="opnsense-tab-backups" hidden={activeTab !== "backups"}>
        <BackupsTab
          profileId={opnsense.selectedProfile?.id ?? null}
          revisions={opnsense.revisions}
          onRefresh={opnsense.refreshRevisions}
          onBackup={opnsense.runBackup}
        />
      </section>

      <section className={styles.tabPanel} id="opnsense-panel-terminal" role="tabpanel" aria-labelledby="opnsense-tab-terminal" hidden={activeTab !== "terminal"}>
        {opnsense.selectedProfile === null ? (
          <p className={styles.hint} data-testid="opnsense-terminal-empty">Select a profile to open a terminal.</p>
        ) : null}
        {[...terminalSlotIds].map((profileId) => {
          const profile = opnsense.profiles.find((item) => item.id === profileId)
          if (profile === undefined) return null
          return (
            <div
              key={profileId}
              data-testid={`opnsense-terminal-slot-${profileId}`}
              hidden={profileId !== opnsense.selectedProfile?.id}
            >
              <OpnsenseTerminalSection
                profile={profile}
                onActivated={activateTerminal}
                onDeactivated={deactivateTerminal}
                onProfilesChanged={opnsense.refreshProfiles}
              />
            </div>
          )
        })}
      </section>

      {terminalSwitchWarning !== null ? (
        <TerminalSwitchWarning
          profileName={terminalSwitchWarning.name}
          profileHost={terminalSwitchWarning.host}
          accent={terminalAccentFor(terminalSwitchWarning.id)}
          onDismiss={(silence) => {
            if (silence) terminalWarningSilencedRef.current = true
            setTerminalSwitchWarning(null)
          }}
        />
      ) : null}
    </section>
  )
}
