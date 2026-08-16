import { expect, test } from "@playwright/test"

test.describe("[ignored] real-network traceroute", () => {
  test.fixme("traceroute reaches a public host", async ({ page }) => {
    await page.goto("/#/traceroute")

    await page.fill('[data-testid="trace-target"]', "example.com")
    await page.click('[data-testid="trace-start"]')

    await expect(page.locator('[data-testid="trace-status"]')).toContainText(
      /completed/i,
    )
  })
})
