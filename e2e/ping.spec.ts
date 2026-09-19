import { expect, test } from "@playwright/test"
import { installMockTauri, makeProbe, type MockProbeEvent } from "./mock-ipc"

declare global {
  interface Window {
    __TAURI_MOCK_SEND_PROBE__: (event: MockProbeEvent, sessionId?: number) => void
    __TAURI_MOCK_SEND_PROBES__: (events: MockProbeEvent[], sessionId?: number) => void
    __TAURI_MOCK_SEND_STATUS_ERROR__: (message: string, sessionId?: number) => void
    __TAURI_MOCK_SET_FALLBACK__: (enabled: boolean) => void
  }
}

test("valid localhost start shows resolved IP and engine status", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')
  await expect(page.locator('[data-testid="resolved-ip"]')).toHaveText(
    "127.0.0.1",
  )
  await expect(page.locator('[data-testid="ping-status"]')).toContainText(
    "Engine: surge",
  )
})

test("invalid input shows inline error and disables Start", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "999.1.1.1")
  await expect(page.locator('[data-testid="ping-target-error"]')).toContainText(
    "invalid IPv4 address",
  )
  await expect(page.locator('[data-testid="ping-start"]')).toBeDisabled()
})

test("600 probes keep the table at 500 rows and show full aggregates", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')

  const probes = Array.from({ length: 600 }, (_, index) =>
    makeProbe(index + 1, 10 + (index % 5), false),
  )
  await page.evaluate((events) => {
    window.__TAURI_MOCK_SEND_PROBES__(events)
  }, probes)

  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 5,
  )

  await page.click('[data-testid="expand-table"]')
  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 500,
  )

  const firstSeq = await page
    .locator('[data-testid="ping-table-row"]')
    .first()
    .getAttribute("data-seq")
  expect(firstSeq).toBe("101")

  await expect(page.locator('[data-testid="aggregates-bar"]')).toContainText(
    "600",
  )
})

test("50% loss session renders lost rows and chart canvases", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')

  const probes = Array.from({ length: 100 }, (_, index) =>
    makeProbe(index + 1, index % 2 === 0 ? 10 : null, index % 2 !== 0),
  )
  await page.evaluate((events) => {
    window.__TAURI_MOCK_SEND_PROBES__(events)
  }, probes)

  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 5,
  )

  await page.click('[data-testid="expand-table"]')
  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 100,
  )

  const lostRows = page.locator('[data-testid="ping-table-row"]').filter({
    hasText: "lost",
  })
  await expect(lostRows).toHaveCount(50)

  await expect(page.locator("canvas")).toHaveCount(3)
})

test("run, stop, list, reopen, and delete a session", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')

  const probes = Array.from({ length: 5 }, (_, index) =>
    makeProbe(index + 1, 10, false),
  )
  await page.evaluate((events) => {
    window.__TAURI_MOCK_SEND_PROBES__(events)
  }, probes)

  await page.click('[data-testid="ping-stop"]')
  await page.waitForSelector('[data-testid="session-item"]')

  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(1)
  await expect(page.locator('[data-testid="session-item"]')).toContainText(
    "5 probes",
  )

  await page.click('[data-testid="session-open"]')
  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 5,
  )
  const firstSeq = await page
    .locator('[data-testid="ping-table-row"]')
    .first()
    .getAttribute("data-seq")
  expect(firstSeq).toBe("1")

  await page.click('[data-testid="session-delete"]')
  await expect(page.locator('[data-testid="confirm-dialog"]')).toBeVisible()
  await page.click('[data-testid="confirm-dialog-confirm"]')
  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(0)
})

test("history caps at five sessions and supports multi-select delete", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")

  for (let index = 0; index < 6; index++) {
    await page.fill('[data-testid="ping-target"]', `cap-host-${index}`)
    await page.click('[data-testid="ping-start"]')
    await page.click('[data-testid="ping-stop"]')
    await expect(
      page.locator('[data-testid="session-item"]').first(),
    ).toContainText(`cap-host-${index}`)
  }

  // Only the five newest sessions are listed, with an expander for the rest.
  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(5)
  const reveal = page.locator('[data-testid="history-reveal"]')
  await expect(reveal).toContainText("Show 1 older")
  await reveal.click()
  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(6)

  // Select two rows and delete them in one confirmed batch.
  await page.click('[data-testid="history-select-toggle"]')
  const checkboxes = page.locator('[data-testid="session-delete-select"]')
  await expect(checkboxes).toHaveCount(6)
  await checkboxes.nth(0).check()
  await checkboxes.nth(1).check()
  await expect(page.locator('[data-testid="history-selection-bar"]')).toContainText(
    "2 selected",
  )
  await page.click('[data-testid="history-delete-selected"]')
  await expect(
    page.locator('[data-testid="confirm-dialog"]'),
  ).toBeVisible()
  await page.click('[data-testid="confirm-dialog-confirm"]')

  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(4)
  await expect(page.locator('[data-testid="history-reveal"]')).toHaveCount(0)
})

test("engine error pauses session and retry falls back", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/")
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')

  await expect(page.locator('[data-testid="ping-status"]')).toContainText(
    "Engine: surge",
  )

  await page.evaluate(() => {
    window.__TAURI_MOCK_SEND_STATUS_ERROR__(
      "engine privileges revoked; retry with fallback",
    )
  })

  await expect(page.locator('[data-testid="pause-banner"]')).toBeVisible()
  await expect(page.locator('[data-testid="pause-message"]')).toContainText(
    "Session paused: engine privileges revoked; retry with fallback",
  )

  await page.evaluate(() => {
    window.__TAURI_MOCK_SET_FALLBACK__(true)
  })
  await page.click('[data-testid="retry-fallback"]')

  await expect(page.locator('[data-testid="ping-status"]')).toContainText(
    "Engine: surge-fallback (fallback)",
  )
})

test("two concurrent sessions keep independent probe counts and snapshots", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")

  // First card
  await page.locator('[data-testid="ping-target"]').nth(0).fill("localhost")
  await page.locator('[data-testid="ping-start"]').nth(0).click()
  await expect(
    page.locator('[data-testid="resolved-ip"]').nth(0),
  ).toHaveText("127.0.0.1")

  // Add a second card
  await page.click('[data-testid="new-ping"]')
  await page.locator('[data-testid="ping-target"]').nth(1).fill("example.com")
  await page.locator('[data-testid="ping-start"]').nth(1).click()
  await expect(
    page.locator('[data-testid="resolved-ip"]').nth(1),
  ).toHaveText("192.0.2.1")

  // Get the two active session ids from the mock.
  const sessionIds = (await page.evaluate(async () => {
    return (await window.__TAURI_INTERNALS__.invoke(
      "list_active_sessions",
      {},
    )) as number[]
  })) as [number, number]
  expect(sessionIds).toHaveLength(2)

  // Send probes to each session independently.
  const firstProbes = Array.from({ length: 3 }, (_, i) =>
    makeProbe(i + 1, 10, false),
  )
  const secondProbes = Array.from({ length: 5 }, (_, i) =>
    makeProbe(i + 1, 20, false),
  )

  await page.evaluate(
    ([events, id]) => {
      window.__TAURI_MOCK_SEND_PROBES__(events, id)
    },
    [firstProbes, sessionIds[0]] as const,
  )
  await page.evaluate(
    ([events, id]) => {
      window.__TAURI_MOCK_SEND_PROBES__(events, id)
    },
    [secondProbes, sessionIds[1]] as const,
  )

  // Each aggregate bar should reflect its own probe count.
  await expect(
    page.locator('[data-testid="aggregates-bar"]').nth(0),
  ).toContainText("3")
  await expect(
    page.locator('[data-testid="aggregates-bar"]').nth(1),
  ).toContainText("5")

  // Stop both sessions.
  await page.locator('[data-testid="ping-stop"]').nth(0).click()
  await page.locator('[data-testid="ping-stop"]').nth(1).click()

  // Both sessions should now appear in the persisted list.
  const ended = (await page.evaluate(async () => {
    return (await window.__TAURI_INTERNALS__.invoke(
      "list_sessions",
      {},
    )) as { id: number; probeCount: number }[]
  }))
  expect(ended.map((s) => s.probeCount).sort()).toEqual([3, 5])
})

test("initial load with past sessions shows empty loader instead of blank card", async ({
  page,
}) => {
  const seed = {
    id: 42,
    targetInput: "localhost",
    resolvedIp: "127.0.0.1",
    family: "auto",
    engine: "surge",
    intervalMs: 1000,
    timeoutMs: 1000,
    payloadSize: 32,
    dontFragment: false,
    startedAt: "2024-01-01T00:00:00.000Z",
    endedAt: "2024-01-01T00:00:05.000Z",
    probeCount: 5,
    lossCount: 0,
    lossPercent: 0,
    probes: [],
  }
  await page.addInitScript((session) => {
    window.__TAURI_MOCK_ENDED_SESSIONS__ = [session]
  }, seed)
  await installMockTauri(page)
  await page.goto("/")

  // No blank card should appear; the empty-state loader should be visible.
  await expect(page.locator('[data-testid="ping-target"]')).toHaveCount(0)
  await expect(
    page.locator('[data-testid="session-panel"]').locator('[data-testid="session-item"]'),
  ).toHaveCount(1)

  // Opening the past session should create a card with the loaded data.
  await page.click('[data-testid="session-open"]')
  await expect(page.locator('[data-testid="resolved-ip"]')).toHaveText(
    "127.0.0.1",
  )
})

test("closing all cards shows past sessions and loading one opens a card", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")

  // Run a short session so there is a past session to load.
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')
  const probes = Array.from({ length: 3 }, (_, i) => makeProbe(i + 1, 10, false))
  await page.evaluate((events) => {
    window.__TAURI_MOCK_SEND_PROBES__(events)
  }, probes)
  await page.click('[data-testid="ping-stop"]')
  await page.waitForSelector('[data-testid="session-item"]')

  // Close the only card; the empty-state panel should appear.
  await page.click('[data-testid="close-ping"]')
  await expect(page.locator('[data-testid="ping-target"]')).toHaveCount(0)
  await expect(
    page.locator('[data-testid="session-panel"]').locator('[data-testid="session-item"]'),
  ).toHaveCount(1)

  // Load the past session from the empty-state panel.
  await page.click('[data-testid="session-open"]')
  await expect(page.locator('[data-testid="resolved-ip"]')).toHaveText(
    "127.0.0.1",
  )
  await page.waitForFunction(
    () =>
      document.querySelectorAll('[data-testid="ping-table-row"]').length === 3,
  )
})

test("deleting a past session from empty-state panel shows a confirmation dialog", async ({
  page,
}) => {
  await installMockTauri(page)
  await page.goto("/")

  // Run and stop a short session.
  await page.fill('[data-testid="ping-target"]', "localhost")
  await page.click('[data-testid="ping-start"]')
  const probes = Array.from({ length: 3 }, (_, i) => makeProbe(i + 1, 10, false))
  await page.evaluate((events) => {
    window.__TAURI_MOCK_SEND_PROBES__(events)
  }, probes)
  await page.click('[data-testid="ping-stop"]')
  await page.waitForSelector('[data-testid="session-item"]')

  // Close the only card to reach the empty-state panel.
  await page.click('[data-testid="close-ping"]')
  await expect(
    page.locator('[data-testid="session-panel"]').locator('[data-testid="session-item"]'),
  ).toHaveCount(1)

  // The in-app confirmation dialog asks before deleting.
  await page.click('[data-testid="session-delete"]')
  const dialog = page.locator('[data-testid="confirm-dialog"]')
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText("Delete this session?")

  await page.click('[data-testid="confirm-dialog-confirm"]')
  await expect(
    page.locator('[data-testid="session-panel"]').locator('[data-testid="session-item"]'),
  ).toHaveCount(0)
})
