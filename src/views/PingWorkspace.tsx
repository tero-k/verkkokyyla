import { useCallback, useState } from "react"
import PingView from "./PingView"

import styles from "./PingWorkspace.module.css"

type SessionTab = {
  id: number
}

let nextId = 1

export default function PingWorkspace() {
  const [sessions, setSessions] = useState<SessionTab[]>([{ id: nextId }])

  const addSession = useCallback(() => {
    nextId += 1
    setSessions((prev) => [...prev, { id: nextId }])
  }, [])

  const closeSession = useCallback((id: number) => {
    setSessions((prev) => prev.filter((s) => s.id !== id))
  }, [])

  return (
    <div className={styles.workspace}>
      <div className={styles.header}>
        <h1>Ping</h1>
        <button
          type="button"
          className={styles.newButton}
          onClick={addSession}
          data-testid="new-ping"
        >
          + New ping
        </button>
      </div>
      <div className={styles.grid}>
        {sessions.map((session) => (
          <PingView key={session.id} onClose={() => closeSession(session.id)} />
        ))}
      </div>
    </div>
  )
}
