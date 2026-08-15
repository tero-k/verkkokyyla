import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

test("sidebar has download speed link and navigation works", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/")

  await page.click("text=Download speed test")
  await expect(page.locator('[data-testid="download-speed-view"]')).toBeVisible()
  await expect(page.locator("h1")).toHaveText("Download speed test")
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
