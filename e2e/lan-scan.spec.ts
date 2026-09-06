import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

test("happy path shows completed scan rows, history, and screenshot", async ({ page }, testInfo) => {
  await installMockTauri(page)
  await page.goto("/#/lan-scan")

  await expect(page.locator('[data-testid="lan-scan-view"]')).toBeVisible()

  const interfaceSelect = page.locator('[data-testid="lan-interface"]')
  await expect(interfaceSelect).toHaveValue("eth0")
  await expect(page.locator('[data-testid="lan-cidr"]')).toHaveValue("192.168.1.10/24")
  await expect(page.locator('[data-testid="lan-tcp-fallback"]')).toBeChecked()

  await page.click('[data-testid="lan-start"]')

  const rows = page.locator('[data-testid="lan-scan-row"]')
  await expect(rows).toHaveCount(2)
  await expect(page.locator('[data-testid="lan-status"]')).toContainText(/completed/i)

  await expect(rows.nth(0)).toContainText("192.168.1.1")
  await expect(rows.nth(0)).toContainText("AA:BB:CC:DD:EE:01")
  await expect(rows.nth(0)).toContainText("Router Corp")
  await expect(rows.nth(1)).toContainText("192.168.1.42")
  await expect(rows.nth(1)).toContainText("AA:BB:CC:DD:EE:02")
  await expect(rows.nth(1)).toContainText("Example Devices")

  await expect(page.locator('[data-testid="lan-progress"]')).toContainText("Scanned 2 of 2 hosts")
  await expect(page.locator('[data-testid="scan-session-panel"]')).toContainText("eth0")
  await expect(page.locator('[data-testid="scan-session-item"]')).toContainText("2 hosts")
  await expect(page.locator('[data-testid="scan-session-item"]')).toContainText("192.168.1.10/24")

  await page.screenshot({ path: testInfo.outputPath("lan-scan-happy.png"), fullPage: true })
})

test("history can reopen and delete a saved scan", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/lan-scan")

  await page.click('[data-testid="lan-start"]')
  const rows = page.locator('[data-testid="lan-scan-row"]')
  await expect(rows).toHaveCount(2)
  await expect(page.locator('[data-testid="lan-status"]')).toContainText(/completed/i)
  await expect(page.locator('[data-testid="scan-session-item"]')).toHaveCount(1)

  await page.click('[data-testid="scan-open"]')
  const reopenedRows = page.locator('[data-testid="lan-scan-row"]')
  await expect(reopenedRows).toHaveCount(2)
  await expect(reopenedRows.nth(0)).toContainText("192.168.1.1")
  await expect(reopenedRows.nth(1)).toContainText("192.168.1.42")

  await page.click('[data-testid="scan-delete"]')
  await expect(page.locator('[data-testid="confirm-dialog"]')).toBeVisible()
  await page.click('[data-testid="confirm-dialog-confirm"]')
  await expect(page.locator('[data-testid="scan-session-item"]')).toHaveCount(0)
  await expect(page.locator('[data-testid="lan-scan-row"]')).toHaveCount(0)
})

test("shows port scan warning banner and consent dialog", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/lan-scan")

  const warning = page.locator('[data-testid="lan-port-scan-warning"]')
  const dialog = page.locator('[data-testid="lan-port-scan-consent-dialog"]')
  const toggle = page.locator('[data-testid="lan-ports-enabled"]')

  await expect(warning).toBeHidden()
  await expect(dialog).toBeHidden()

  await toggle.check()

  await expect(dialog).toBeVisible()
  await page.locator('[data-testid="lan-port-scan-consent-dialog"] button', { hasText: "Confirm" }).click()

  await expect(dialog).toBeHidden()
  await expect(warning).toBeVisible()

  await toggle.uncheck()
  await expect(warning).toBeHidden()
})

test("port scan toggle adds ports column", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/lan-scan")

  const toggle = page.locator('[data-testid="lan-ports-enabled"]')
  const dialog = page.locator('[data-testid="lan-port-scan-consent-dialog"]')

  await toggle.check()
  await expect(dialog).toBeVisible()
  await page.locator('[data-testid="lan-port-scan-consent-dialog"] button', { hasText: "Confirm" }).click()
  await expect(dialog).toBeHidden()

  await page.click('[data-testid="lan-start"]')

  const rows = page.locator('[data-testid="lan-scan-row"]')
  await expect(rows).toHaveCount(2)

  const firstRow = rows.nth(0)
  const secondRow = rows.nth(1)

  await expect(firstRow).toContainText("192.168.1.1")
  await expect(firstRow).toContainText("-")

  await expect(secondRow).toContainText("192.168.1.42")
  await expect(secondRow).toContainText("22 ssh")
  await expect(secondRow).toContainText("80 http")
})
