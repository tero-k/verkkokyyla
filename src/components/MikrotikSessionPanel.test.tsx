// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MikrotikSessionSummaryDto } from "../lib/types"
import { MikrotikSessionPanel } from "./MikrotikSessionPanel"

const sessions: readonly MikrotikSessionSummaryDto[] = [
  {
    id: 22,
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
  },
]

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
})
