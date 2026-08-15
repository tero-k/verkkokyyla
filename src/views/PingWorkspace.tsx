import { useCallback, useEffect, useState } from "react"
import PingView from "./PingView"
import { PingSessionPanel } from "../components/PingSessionPanel"
import { deleteSession, listSessions } from "../lib/ipc"
import type { SessionSummaryDto } from "../lib/types"

import styles from "./PingWorkspace.module.css"

type SessionTab = {
  id: number
  initialSessionId?: number
}

let nextId = 1

export default function PingWorkspace() {
  const [tabs, setTabs] = useState<SessionTab[]>([{ id: nextId }])
  const [pastSessions, setPastSessions] = useState<SessionSummaryDto[]>([])

  const refreshPast = useCallback(async () => {
    try {
      const sessions = await listSessions()
      setPastSessions(sessions)
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("failed to load past sessions", err)
    }
  }, [])

  useEffect(() => {
    void refreshPast()
  }, [refreshPast])

  const addSession = useCallback(() => {
    nextId += 1
    setTabs((prev) => [...prev, { id: nextId }])
  }, [])

  const openPastSession = useCallback((id: number) => {
    nextId += 1
    setTabs((prev) => [...prev, { id: nextId, initialSessionId: id }])
  }, [])

  const closeSession = useCallback((id: number) => {
    setTabs((prev) => prev.filter((s) => s.id !== id))
  }, [])

  const handleDelete = useCallback(
    async (id: number) => {
      await deleteSession(id)
      void refreshPast()
    },
    [refreshPast],
  )

  const isEmpty = tabs.length === 0

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
      {isEmpty ? (
        <div className={styles.emptyState}>
          <p className={styles.emptyHint}>No active pings.</p>
          <div className={styles.emptyPanel}>
            <PingSessionPanel
              sessions={pastSessions}
              disabled={false}
              onOpen={openPastSession}
              onDelete={handleDelete}
            />
          </div>
        </div>
      ) : (
        <div className={styles.grid}>
          {tabs.map((tab) => (
            <PingView
              key={tab.id}
              onClose={() => closeSession(tab.id)}
              initialSessionId={tab.initialSessionId}
            />
          ))}
        </div>
      )}
    </div>
  )
}
