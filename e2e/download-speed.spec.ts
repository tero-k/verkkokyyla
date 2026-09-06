import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

test("sidebar has web page speed test link and navigation works", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/")

  await page.click("text=Web Benchmark")
  await expect(page.locator('[data-testid="download-speed-view"]')).toBeVisible()
  await expect(page.locator("h1")).toHaveText("Web Benchmark")
})

test("invalid URL disables start and shows inline error", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.fill('[data-testid="download-url"]', "not-a-url")
  await expect(page.locator('[data-testid="download-start"]')).toBeDisabled()
  await expect(page.locator('[data-testid="download-error"]')).toHaveCount(0)
})

test("successful mocked download shows progress and final stats", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.fill('[data-testid="download-url"]', "https://example.test/file.bin")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("Average speed")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("8.39 Mbps")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("200")
  await expect(page.locator('[data-testid="download-start"]')).not.toBeDisabled()
})

test("successful mocked full-page test shows resource table", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.fill('[data-testid="download-url"]', "https://example.test/page")
  await page.selectOption('[data-testid="download-mode"]', "page")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("Resources")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("6/6")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("Total time")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("565 ms")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("Time to first byte")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("40 ms")
  await expect(
    page.locator('[data-testid="page-resource-table"]'),
  ).toContainText("style.css")
  await expect(
    page.locator('[data-testid="page-resource-table"]'),
  ).toContainText("app.js")
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("Top 5 slowest resources are highlighted")
  await expect(
    page.locator('[data-testid="page-resource-table"]'),
  ).toContainText("#1 slowest")

  const slowRows = page.locator('[data-testid="page-resource-table"] tbody tr[data-slow="true"]')
  await expect(slowRows).toHaveCount(5)

  await expect(page.locator('[data-testid="download-start"]')).not.toBeDisabled()
})

test("mocked backend error renders error banner", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.fill('[data-testid="download-url"]', "https://error.test/")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-error"]')).toContainText(
    "mock failure",
  )
  await expect(page.locator('[data-testid="download-results"]')).toHaveCount(0)
})

test("HTTP settings panel can be opened and custom user agent is sent", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.click('[data-testid="http-settings-panel"] summary')
  await expect(page.locator('[data-testid="http-user-agent"]')).toBeVisible()

  await page.fill('[data-testid="http-user-agent"]', "custom-test-agent/1.0")
  await page.fill('[data-testid="download-url"]', "https://example.test/file.bin")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
  const settings = await page.evaluate(
    () => window.__TAURI_MOCK_LAST_HTTP_SETTINGS__,
  )
  expect(settings).not.toBeNull()
  expect(settings?.userAgent).toBe("custom-test-agent/1.0")
})

test("HTTP settings compression toggle is sent to backend", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.click('[data-testid="http-settings-panel"] summary')
  await page.uncheck('[data-testid="http-compression"]')
  await page.selectOption('[data-testid="download-mode"]', "page")
  await page.fill('[data-testid="download-url"]', "https://example.test/page")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
  const settings = await page.evaluate(
    () => window.__TAURI_MOCK_LAST_HTTP_SETTINGS__,
  )
  expect(settings).not.toBeNull()
  expect(settings?.compression).toBe(false)
})

test("successful mocked download saves a history entry, reopens it, and deletes it", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/download-speed")

  await page.fill('[data-testid="download-url"]', "https://example.test/file.bin")
  await page.click('[data-testid="download-start"]')

  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()

  const items = page.locator('[data-testid="download-speed-session-item"]')
  await expect(items).toHaveCount(1)
  await expect(
    page.locator('[data-testid="download-speed-session-panel"]'),
  ).toContainText("https://example.test/file.bin")

  await page.click('[data-testid="download-reset"]')
  await expect(page.locator('[data-testid="download-results"]')).toHaveCount(0)

  await page
    .locator('[data-testid="download-speed-open"]')
    .first()
    .click()
  await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
  await expect(
    page.locator('[data-testid="download-results"]'),
  ).toContainText("8.39 Mbps")

  await page
    .locator('[data-testid="download-speed-delete"]')
    .first()
    .click()
  await expect(page.locator('[data-testid="confirm-dialog"]')).toBeVisible()
  await expect(page.locator('[data-testid="confirm-dialog"]')).toContainText("Delete this speed test?")
  await page.click('[data-testid="confirm-dialog-confirm"]')
  await expect(page.locator('[data-testid="download-speed-session-item"]')).toHaveCount(0)
  await expect(
    page.locator('[data-testid="download-speed-session-panel"]'),
  ).toContainText("No saved speed tests yet.")
})
