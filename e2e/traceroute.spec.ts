import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

test("happy path shows completed traceroute rows and screenshot", async ({ page }, testInfo) => {
  await installMockTauri(page)
  await page.goto("/#/traceroute")

  await page.fill('[data-testid="trace-target"]', "example.com")
  await page.click('[data-testid="trace-start"]')

  await expect(page.locator('[data-testid="trace-status"]')).toContainText(/completed/i)

  const rows = page.locator('[data-testid="trace-row"]')
  await expect(rows).toHaveCount(3)
  await expect(rows.nth(0)).toContainText("192.0.2.1")
  await expect(rows.nth(1)).toContainText("198.51.100.1")
  await expect(rows.nth(2)).toContainText("203.0.113.9")
  await expect(rows.nth(2).locator("td").nth(2)).not.toHaveText("-")

  await expect(page.locator('[data-testid="trace-progress"]')).toContainText("Hop 3/30")
  await expect(page.locator('[data-testid="trace-session-panel"]')).toContainText("example.com")
  await expect(page.locator('[data-testid="trace-session-item"]')).toContainText("3 hops")
  await expect(page.locator('[data-testid="trace-session-item"]')).toContainText("203.0.113.9")

  await page.screenshot({ path: testInfo.outputPath("traceroute-happy.png"), fullPage: true })
})

test("error path shows unavailable message and no traces", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/traceroute")

  await page.fill('[data-testid="trace-target"]', "error.test")
  await page.click('[data-testid="trace-start"]')

  await expect(page.locator('[data-testid="trace-error"]')).toContainText(/Traceroute binary not found/i)
  await expect(page.locator('[data-testid="trace-row"]')).toHaveCount(0)
  await expect(page.locator('[data-testid="trace-session-item"]')).toHaveCount(0)
})

test("history can reopen and delete a saved trace", async ({ page }) => {
  await installMockTauri(page)
  page.on("dialog", (dialog) => void dialog.accept())
  await page.goto("/#/traceroute")

  await page.fill('[data-testid="trace-target"]', "example.com")
  await page.click('[data-testid="trace-start"]')

  await expect(page.locator('[data-testid="trace-status"]')).toContainText(/completed/i)
  await expect(page.locator('[data-testid="trace-session-item"]')).toHaveCount(1)

  await page.click('[data-testid="trace-open"]')
  const reopenedRows = page.locator('[data-testid="trace-row"]')
  await expect(reopenedRows).toHaveCount(3)
  await expect(reopenedRows.nth(0)).toContainText("192.0.2.1")
  await expect(reopenedRows.nth(1)).toContainText("198.51.100.1")
  await expect(reopenedRows.nth(2)).toContainText("203.0.113.9")

  await page.click('[data-testid="trace-delete"]')
  await expect(page.locator('[data-testid="trace-session-item"]')).toHaveCount(0)
})
