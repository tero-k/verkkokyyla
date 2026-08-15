import { useEffect, useState } from "react"
import DownloadSpeedView from "./views/DownloadSpeedView"
import PingWorkspace from "./views/PingWorkspace"
import { NAV_ITEMS, resolveRoute } from "./routing"
import "./App.css"
import styles from "./App.module.css"

export default function App() {
  const [hash, setHash] = useState(window.location.hash)
  const route = resolveRoute(hash)

  useEffect(() => {
    const onHashChange = () => setHash(window.location.hash)
    window.addEventListener("hashchange", onHashChange)
    return () => window.removeEventListener("hashchange", onHashChange)
  }, [])

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>
        <div className={styles.brand}>verkkokyyla</div>
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
      </aside>
      <main className={styles.content}>
        {route === "download-speed" ? <DownloadSpeedView /> : <PingWorkspace />}
      </main>
    </div>
  )
}
