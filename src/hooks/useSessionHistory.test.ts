// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react"
import { describe, expect, it } from "vitest"
import { HISTORY_PAGE_SIZE } from "../lib/constants"
import { useSessionHistory } from "./useSessionHistory"

type Row = { readonly id: number }

function makeRows(count: number): Row[] {
  return Array.from({ length: count }, (_, index) => ({ id: index + 1 }))
}

describe("useSessionHistory", () => {
  it("caps the visible list at the newest entries", () => {
    const { result } = renderHook(() => useSessionHistory(makeRows(7)))

    expect(result.current.visible.map((row) => row.id)).toEqual([1, 2, 3, 4, 5])
    expect(result.current.hiddenCount).toBe(2)
    expect(result.current.expanded).toBe(false)
  })

  it("shows everything once expanded and collapses again", () => {
    const { result } = renderHook(() => useSessionHistory(makeRows(7)))

    act(() => result.current.toggleExpanded())
    expect(result.current.visible).toHaveLength(7)
    expect(result.current.hiddenCount).toBe(0)

    act(() => result.current.toggleExpanded())
    expect(result.current.visible).toHaveLength(HISTORY_PAGE_SIZE)
    expect(result.current.hiddenCount).toBe(2)
  })

  it("does not cap lists at or below the page size", () => {
    const { result } = renderHook(() => useSessionHistory(makeRows(3)))

    expect(result.current.visible).toHaveLength(3)
    expect(result.current.hiddenCount).toBe(0)
  })

  it("tracks selection toggles and clears on exit", () => {
    const { result } = renderHook(() => useSessionHistory(makeRows(3)))

    act(() => result.current.enterSelectMode())
    act(() => result.current.toggleSelected(1))
    act(() => result.current.toggleSelected(2))
    expect(result.current.selectedCount).toBe(2)

    act(() => result.current.toggleSelected(1))
    expect(result.current.selectedCount).toBe(1)
    expect(result.current.selectedIds.has(2)).toBe(true)

    act(() => result.current.exitSelectMode())
    expect(result.current.selectMode).toBe(false)
    expect(result.current.selectedCount).toBe(0)
  })

  it("drops selections for sessions that disappear from the list", () => {
    const { result, rerender } = renderHook(
      ({ rows }) => useSessionHistory(rows),
      { initialProps: { rows: makeRows(3) } },
    )

    act(() => result.current.toggleSelected(2))
    rerender({ rows: makeRows(3).filter((row) => row.id !== 2) })

    expect(result.current.selectedCount).toBe(0)
  })
})
