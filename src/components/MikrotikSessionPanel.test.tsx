// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MikrotikSessionSummaryDto } from "../lib/types"
import { MikrotikSessionPanel } from "./MikrotikSessionPanel"

function makeSession(id: number): MikrotikSessionSummaryDto {
  return {
    id,
    profileId: 7,
    startedAt: "2026-09-06T12:00:00Z",
    endedAt: "2026-09-06T12:05:00Z",
    status: "completed",
    boardName: "RB5009",
    routerosVersion: "7.16",
    architectureName: "arm64",
    updateStatusJson: null,
    firmwareStatusJson: null,
    snapshotCount: 12,
  }
}

const sessions: readonly MikrotikSessionSummaryDto[] = [makeSession(22)]

afterEach(cleanup)

describe("MikrotikSessionPanel", () => {
  it("renders saved sessions with load and delete actions", () => {
    const onOpen = vi.fn<(id: number) => void>()
    const onDelete = vi.fn<(id: number) => void>()

    render(
      <MikrotikSessionPanel
        sessions={sessions}
        disabled={false}
        onOpen={onOpen}
        onDelete={onDelete}
      />,
    )

    const item = screen.getByTestId("mikrotik-session-item")
    expect(within(item).getByText("RB5009 / 7.16")).toBeTruthy()
    expect(within(item).getByText("12 snapshots - completed")).toBeTruthy()

    fireEvent.click(within(item).getByTestId("mikrotik-open-session"))
    fireEvent.click(within(item).getByTestId("mikrotik-delete-session"))

    expect(onOpen).toHaveBeenCalledWith(22)
    expect(onDelete).toHaveBeenCalledWith(22)
  })

  it("disables actions while monitoring is running", () => {
    render(
      <MikrotikSessionPanel
        sessions={sessions}
        disabled={true}
        onOpen={() => undefined}
        onDelete={() => undefined}
      />,
    )

    expect(screen.getByTestId("mikrotik-open-session").hasAttribute("disabled")).toBe(true)
    expect(screen.getByTestId("mikrotik-delete-session").hasAttribute("disabled")).toBe(true)
  })

  it("renders an empty state when no sessions are saved", () => {
    render(
      <MikrotikSessionPanel
        sessions={[]}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
      />,
    )

    expect(screen.getByText("No saved MikroTik sessions yet.").className).toMatch(/empty/)
  })

  it("shows only the five newest sessions until the older ones are revealed", () => {
    const many = Array.from({ length: 7 }, (_, index) => makeSession(index + 1))

    render(
      <MikrotikSessionPanel
        sessions={many}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
      />,
    )

    expect(screen.getAllByTestId("mikrotik-session-item")).toHaveLength(5)
    expect(screen.getByTestId("history-reveal").textContent).toContain("Show 2 older")
  })

  it("reveals older sessions on demand and collapses again", () => {
    const many = Array.from({ length: 7 }, (_, index) => makeSession(index + 1))

    render(
      <MikrotikSessionPanel
        sessions={many}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
      />,
    )

    fireEvent.click(screen.getByTestId("history-reveal"))
    expect(screen.getAllByTestId("mikrotik-session-item")).toHaveLength(7)

    fireEvent.click(screen.getByTestId("history-reveal"))
    expect(screen.getAllByTestId("mikrotik-session-item")).toHaveLength(5)
  })

  it("bulk-deletes the selected sessions through onDeleteMany", async () => {
    const many = Array.from({ length: 3 }, (_, index) => makeSession(index + 1))
    const onDeleteMany = vi.fn<(ids: readonly number[]) => void>()

    render(
      <MikrotikSessionPanel
        sessions={many}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
        onDeleteMany={onDeleteMany}
      />,
    )

    fireEvent.click(screen.getByTestId("history-select-toggle"))

    const checkboxes = screen.getAllByTestId("mikrotik-delete-select")
    expect(checkboxes).toHaveLength(3)
    fireEvent.click(checkboxes[0])
    fireEvent.click(checkboxes[2])

    expect(screen.getByTestId("history-selection-bar").textContent).toContain("2 selected")

    fireEvent.click(screen.getByTestId("history-delete-selected"))
    expect(onDeleteMany).toHaveBeenCalledWith([1, 3])

    await waitFor(() => {
      expect(screen.queryByTestId("history-selection-bar")).toBeNull()
    })
  })

  it("exits select mode on cancel and hides the toggle without onDeleteMany", () => {
    const withDeleteMany = render(
      <MikrotikSessionPanel
        sessions={sessions}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
        onDeleteMany={() => undefined}
      />,
    )

    fireEvent.click(withDeleteMany.getByTestId("history-select-toggle"))
    expect(withDeleteMany.getByTestId("history-selection-bar")).toBeTruthy()
    fireEvent.click(withDeleteMany.getByTestId("history-selection-cancel"))
    expect(withDeleteMany.queryByTestId("history-selection-bar")).toBeNull()
    withDeleteMany.unmount()

    render(
      <MikrotikSessionPanel
        sessions={sessions}
        disabled={false}
        onOpen={() => undefined}
        onDelete={() => undefined}
      />,
    )
    expect(screen.queryByTestId("history-select-toggle")).toBeNull()
  })
})
