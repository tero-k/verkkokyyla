import { useEffect, useState } from "react"
import DownloadSpeedView from "./views/DownloadSpeedView"
import DnsTesterView from "./views/DnsTesterView"
import LanScanView from "./views/LanScanView"
import MikrotikView from "./views/MikrotikView"
import PingWorkspace from "./views/PingWorkspace"
import TracerouteView from "./views/TracerouteView"
import { NAV_ITEMS, resolveRoute } from "./routing"
import { useTheme, type ThemeMode } from "./theme"
import "./App.css"
import styles from "./App.module.css"

export default function App() {
  const [hash, setHash] = useState(window.location.hash)
  const route = resolveRoute(hash)
  const { mode, setMode } = useTheme()

  useEffect(() => {
    const onHashChange = () => setHash(window.location.hash)
    window.addEventListener("hashchange", onHashChange)
    return () => window.removeEventListener("hashchange", onHashChange)
  }, [])

  const modes: ThemeMode[] = ["light", "dark", "system"]

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>
        <div className={styles.brand}>Verkkokyylä</div>
        <nav aria-label="Tools">
          <ul className={styles.navList}>
            {NAV_ITEMS.map((item) => (
              <li key={item.route}>
                <a
                  className={`${styles.navItem}${item.route === route ? ` ${styles.navItemActive}` : ""}`}
                  href={item.hash}
                >
                  {item.label}
                </a>
              </li>
            ))}
          </ul>
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
        {route === "lan-scan" && <LanScanView />}
        {route === "dns-tester" && <DnsTesterView />}
        {route === "mikrotik" && <MikrotikView />}
      </main>
    </div>
  )
}
