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
  const [tabs, setTabs] = useState<SessionTab[]>([])
  const [pastSessions, setPastSessions] = useState<SessionSummaryDto[]>([])
  const [ready, setReady] = useState(false)

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
    let mounted = true
    const init = async () => {
      try {
        const sessions = await listSessions()
        if (!mounted) return
        setPastSessions(sessions)
        if (sessions.length === 0) {
          nextId += 1
          setTabs([{ id: nextId }])
        }
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error("failed to initialize workspace", err)
        nextId += 1
        setTabs([{ id: nextId }])
      } finally {
        if (mounted) setReady(true)
      }
    }
    void init()
    return () => {
      mounted = false
    }
  }, [])

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
      {!ready ? (
        <p className={styles.loading}>Loading…</p>
      ) : isEmpty ? (
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
