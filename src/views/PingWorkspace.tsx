import { useCallback, useEffect, useState } from "react"
import PingView from "./PingView"
import { PingSessionPanel } from "../components/PingSessionPanel"
import { deleteSession, listSessions } from "../lib/ipc"
import { useConfirmDialog } from "../hooks/useConfirmDialog"
import type { SessionSummaryDto } from "../lib/types"
import { Button, ViewHeader } from "../components/ui/ui"

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
  const { confirm, dialog: confirmDialog } = useConfirmDialog()

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

  // Refresh the persisted list whenever the workspace falls back to the
  // empty state (e.g. the last card was closed): sessions stopped in the
  // meantime only exist in the database, not in this component's state.
  useEffect(() => {
    if (ready && tabs.length === 0) {
      void refreshPast()
    }
  }, [ready, tabs.length, refreshPast])

  const openPastSession = useCallback((id: number) => {
    nextId += 1
    setTabs((prev) => [...prev, { id: nextId, initialSessionId: id }])
  }, [])

  const closeSession = useCallback((id: number) => {
    setTabs((prev) => prev.filter((s) => s.id !== id))
  }, [])

  const handleDelete = useCallback(
    async (id: number) => {
      if (!(await confirm("Delete this session?"))) return
      await deleteSession(id)
      void refreshPast()
    },
    [refreshPast, confirm],
  )

  const handleDeleteMany = useCallback(
    async (ids: readonly number[]) => {
      if (ids.length === 0) return
      if (!(await confirm(`Delete ${ids.length} sessions?`))) return
      await Promise.all(ids.map((id) => deleteSession(id)))
      void refreshPast()
    },
    [refreshPast, confirm],
  )

  const isEmpty = tabs.length === 0

  return (
    <div className={styles.workspace}>
      <ViewHeader
        title={<h1 className={styles.title}>Ping / ICMP</h1>}
        subtitle={ready ? `${tabs.length} active workspace${tabs.length === 1 ? "" : "s"}` : "Preparing workspace"}
      >
        <div className="vk-view-actions">
          <Button
            variant="primary"
            onClick={addSession}
            data-testid="new-ping"
          >
            New ping
          </Button>
        </div>
      </ViewHeader>

      <div className={styles.body}>
        {!ready ? (
          <p className={styles.loading}>Loading ping history…</p>
        ) : isEmpty ? (
          <div className={styles.emptyState}>
            <div className={styles.emptyCopy}>
              <span className={styles.eyebrow}>Ping workspace</span>
              <h2>No active pings</h2>
              <p className={styles.emptyHint}>
                Start a new ICMP session or reopen a saved run below.
              </p>
            </div>
            <div className={styles.emptyPanel}>
              <PingSessionPanel
                sessions={pastSessions}
                disabled={false}
                onOpen={openPastSession}
                onDelete={handleDelete}
                onDeleteMany={handleDeleteMany}
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
      {confirmDialog}
    </div>
  )
}
