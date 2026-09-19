import { useEffect, useState } from "react"
import DownloadSpeedView from "./views/DownloadSpeedView"
import DnsTesterView from "./views/DnsTesterView"
import HelpView from "./views/HelpView"
import LanScanView from "./views/LanScanView"
import MikrotikView from "./views/MikrotikView"
import MtuView from "./views/MtuView"
import PingWorkspace from "./views/PingWorkspace"
import TracerouteView from "./views/TracerouteView"
import { NAV_GROUPS, NAV_ITEMS, resolveRoute } from "./routing"
import { useTheme, type ThemeMode } from "./theme"
import "./App.css"
import styles from "./App.module.css"

const LABELS = new Map(NAV_ITEMS.map((i) => [i.route, i]))

export default function App() {
  const [hash, setHash] = useState(window.location.hash)
  const route = resolveRoute(hash)
  const { mode, setMode } = useTheme()
  // Becomes true on the first visit and never goes back — the keep-alive
  // switch for MikroTik's long-running sessions.
  const [mikrotikVisited, setMikrotikVisited] = useState(route === "mikrotik")

  useEffect(() => {
    if (route === "mikrotik") setMikrotikVisited(true)
  }, [route])

  useEffect(() => {
    const onHashChange = () => setHash(window.location.hash)
    window.addEventListener("hashchange", onHashChange)
    return () => window.removeEventListener("hashchange", onHashChange)
  }, [])

  // Ctrl+digit navigation, as hinted by the sidebar kbd labels.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return
      const digit = Number.parseInt(e.key, 10)
      if (!Number.isInteger(digit) || digit < 1) return
      const flat = NAV_GROUPS.flatMap((g) => g.items)
      const target = flat[digit - 1]
      if (!target) return
      const item = LABELS.get(target.route)
      if (!item) return
      e.preventDefault()
      window.location.hash = item.hash
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [])

  const modes: ThemeMode[] = ["light", "dark", "system"]

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>
        <div className={styles.brand}>
          <span className={styles.brandMark} />
          <span className={styles.brandName}>Verkkokyylä</span>
        </div>
        <nav className={styles.nav} aria-label="Tools">
          {NAV_GROUPS.map((group) => (
            <div key={group.label} className={styles.navGroup}>
              <div className={styles.navGroupLabel}>{group.label}</div>
              <ul className={styles.navList}>
                {group.items.map(({ route: r, shortcut }) => {
                  const item = LABELS.get(r)
                  if (!item) return null
                  const active = r === route
                  return (
                    <li key={r}>
                      <a
                        className={`${styles.navItem}${active ? ` ${styles.navItemActive}` : ""}`}
                        href={item.hash}
                        aria-current={active ? "page" : undefined}
                      >
                        <span className={styles.navDot} />
                        <span className={styles.navName}>{item.label}</span>
                        <span className={styles.navKbd}>{shortcut}</span>
                      </a>
                    </li>
                  )
                })}
              </ul>
            </div>
          ))}
        </nav>
        <div className={styles.themeSwitcher} role="group" aria-label="Theme">
          {modes.map((m) => (
            <button
              key={m}
              type="button"
              aria-pressed={mode === m}
              className={`${styles.themeButton}${mode === m ? ` ${styles.themeButtonActive}` : ""}`}
              onClick={() => setMode(m)}
            >
              {m === "system" ? "OS" : m.charAt(0).toUpperCase() + m.slice(1)}
            </button>
          ))}
        </div>
      </aside>
      <main className={styles.content}>
        {route === "ping" && <PingWorkspace />}
        {route === "download-speed" && <DownloadSpeedView />}
        {route === "traceroute" && <TracerouteView />}
        {route === "mtu" && <MtuView />}
        {route === "lan-scan" && <LanScanView />}
        {route === "dns-tester" && <DnsTesterView />}
        {route === "help" && <HelpView />}
        {/* MikroTik mounts on first visit, then stays mounted across routes:
            monitoring sessions, log streams, and SSH terminals keep
            collecting while the user works in Measure or Discover.
            display:contents keeps the wrapper layout-neutral; display:none
            detaches it from paint without unmounting, so channels and xterm
            state survive navigation. Lazy mounting keeps the MikroTik DOM
            and its startup IPC out of sessions that never open the tool. */}
        {mikrotikVisited ? (
          <div style={{ display: route === "mikrotik" ? "contents" : "none" }}>
            <MikrotikView />
          </div>
        ) : null}
      </main>
    </div>
  )
}
