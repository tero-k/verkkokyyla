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
  ).toContainText("7/7")
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

  const resourceRows = page.locator('[data-testid="page-resource-table"] tbody tr')
  await expect(resourceRows).toHaveCount(7)
  await expect(resourceRows.nth(0)).toContainText("https://example.test/page")
  await expect(resourceRows.nth(1)).toContainText("style.css")

  const durationSort = page.getByRole("button", { name: /Duration/ })
  await durationSort.click()
  await expect(resourceRows.first()).toContainText("image.png")
  await durationSort.click()
  await expect(resourceRows.first()).toContainText("xhr.json")
  await durationSort.click()

  // Regression: two mock resources share the page URL (document + a
  // self-referencing link). Repeated re-sorts must never duplicate rows or
  // the #1 slowest badge (duplicate React keys caused row cloning).
  for (const name of [/Type/, /Status/, /URL/, /Duration/, /Duration/]) {
    await page.getByRole("button", { name }).click()
  }
  await expect(resourceRows).toHaveCount(7)
  const sharedUrlRows = page.locator('[data-testid="page-resource-table"] tbody tr', {
    hasText: "https://example.test/page",
  })
  await expect(sharedUrlRows).toHaveCount(2)
  await expect(
    page.locator('[aria-label="Slowest resource rank 1"]'),
  ).toHaveCount(1)

  const scriptFilter = page.getByRole("button", { name: "script", exact: true })
  await scriptFilter.click()
  await expect(resourceRows).toHaveCount(6)
  await expect(page.locator('[data-testid="page-resource-table"]')).not.toContainText("app.js")
  await expect(page.locator('[data-testid="download-results"]')).toContainText(
    "Showing 6 of 7 resources",
  )

  const statusFilter = page.getByRole("combobox", { name: "Status" })
  await statusFilter.selectOption("4xx")
  await expect(resourceRows).toHaveCount(0)
  await expect(page.locator('[data-testid="download-results"]')).toContainText(
    "Showing 0 of 7 resources",
  )

  await statusFilter.selectOption("all")
  await scriptFilter.click()
  const slowestOnly = page.getByRole("button", { name: "Slowest only" })
  await slowestOnly.click()
  await expect(slowestOnly).toHaveAttribute("aria-pressed", "true")
  await expect(slowestOnly).toHaveClass(/typeChipActive/)
  // Both active chips share .typeChipActive, whose background transitions over
  // 100ms — poll both sides so a mid-transition capture cannot flake.
  await expect
    .poll(async () => {
      const typeBackground = await scriptFilter.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      )
      const slowestBackground = await slowestOnly.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      )
      return slowestBackground === typeBackground
    })
    .toBe(true)
  await expect(resourceRows).toHaveCount(5)

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
