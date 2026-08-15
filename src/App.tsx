import "./App.css";
import styles from "./App.module.css";
import PingWorkspace from "./views/PingWorkspace";
import { NAV_ITEMS, resolveRoute } from "./routing";

export default function App() {
  const route = resolveRoute(window.location.hash);

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
        <PingWorkspace />
      </main>
    </div>
  );
}
