import { useCallback, useEffect, useMemo, useState } from "react"
import { HISTORY_PAGE_SIZE } from "../lib/constants"

type SessionLike = { readonly id: number }

/**
 * Shared behavior for the history lists (ping, traceroute, speed tests,
 * scans, MikroTik sessions, MTU runs, DNS runs):
 *
 * - caps the rendered list at the newest `initialCount` entries and exposes
 *   an expand/collapse toggle for the older ones,
 * - maintains an opt-in multi-select mode used for bulk deletion.
 *
 * Lists come from the backend ordered newest-first, so "latest" is the head
 * of the array. Selection state is local to the panel and pruned
 * automatically when entries disappear from the list.
 */
export function useSessionHistory<T extends SessionLike>(
  sessions: readonly T[],
  initialCount: number = HISTORY_PAGE_SIZE,
) {
  const [expanded, setExpanded] = useState(false)
  const [selectMode, setSelectMode] = useState(false)
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<number>>(
    () => new Set<number>(),
  )

  const visibleCount = expanded
    ? sessions.length
    : Math.min(initialCount, sessions.length)
  const visible = useMemo(
    () => sessions.slice(0, visibleCount),
    [sessions, visibleCount],
  )
  const hiddenCount = sessions.length - visible.length

  const toggleExpanded = useCallback(() => setExpanded((value) => !value), [])

  const enterSelectMode = useCallback(() => {
    setSelectMode(true)
    setSelectedIds(new Set<number>())
  }, [])

  const exitSelectMode = useCallback(() => {
    setSelectMode(false)
    setSelectedIds(new Set<number>())
  }, [])

  const toggleSelected = useCallback((id: number) => {
    setSelectedIds((current) => {
      const next = new Set(current)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  // Drop selections pointing at sessions that disappeared (deleted via the
  // per-row action or a refreshed list) so the count stays truthful.
  useEffect(() => {
    setSelectedIds((current) => {
      const valid = new Set(sessions.map((session) => session.id))
      const kept = new Set<number>()
      for (const id of current) {
        if (valid.has(id)) kept.add(id)
      }
      return kept.size === current.size ? current : kept
    })
  }, [sessions])

  return {
    visible,
    hiddenCount,
    expanded,
    toggleExpanded,
    selectMode,
    selectedIds,
    selectedCount: selectedIds.size,
    toggleSelected,
    enterSelectMode,
    exitSelectMode,
  }
}
