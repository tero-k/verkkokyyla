import { expect, test } from "@playwright/test"
import { installMockTauri, makeProbe, type MockProbeEvent } from "./mock-ipc"

declare global {
  interface Window {
    __TAURI_MOCK_SEND_PROBE__: (event: MockProbeEvent) => void
    __TAURI_MOCK_SEND_PROBES__: (events: MockProbeEvent[]) => void
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
  page.on("dialog", (dialog) => void dialog.accept())
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
  await expect(page.locator('[data-testid="session-item"]')).toHaveCount(0)
})
