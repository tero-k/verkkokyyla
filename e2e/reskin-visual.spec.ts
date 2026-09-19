// Visual QA spec for the Claude Design reskin: captures populated views in
// both themes to .omo/evidence (mirrors the f5-visual.spec.ts precedent).
import { expect, test, type Page } from "@playwright/test"
import { mkdirSync } from "node:fs"
import path from "node:path"
import { installMockTauri } from "./mock-ipc"

test.use({ viewport: { width: 1280, height: 800 } })

const evidenceDir = path.join(process.cwd(), ".omo", "evidence")

function shot(fileName: string): string {
  mkdirSync(evidenceDir, { recursive: true })
  return path.join(evidenceDir, fileName)
}

async function setTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.evaluate((t) => {
    localStorage.setItem("verkkokyyla-theme", t)
    document.documentElement.dataset.theme = t
    document.documentElement.style.colorScheme = t
  }, theme)
  // Force theme-dependent graphs to re-read tokens.
  await page.reload()
}

for (const theme of ["light", "dark"] as const) {
  test(`reskin visual: ping populated (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/ping")
    await setTheme(page, theme)
    await page.fill('[data-testid="ping-target"]', "localhost")
    await page.click('[data-testid="ping-start"]')
    await expect(page.locator('[data-testid="resolved-ip"]')).toHaveText("127.0.0.1")
    await page.evaluate(() => {
      const now = Date.now()
      window.__TAURI_MOCK_SEND_PROBES__(
        Array.from({ length: 30 }, (_, i) => ({
          seq: i + 1,
          rttMs: i % 9 === 4 ? null : 0.8 + ((i * 7) % 13) / 10,
          lost: i % 9 === 4,
          at: new Date(now - (30 - i) * 1000).toISOString(),
        })),
      )
    })
    await page.waitForTimeout(800)
    await page.screenshot({ path: shot(`reskin-ping-${theme}.png`), fullPage: true })
  })

  test(`reskin visual: traceroute populated (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/traceroute")
    await setTheme(page, theme)
    await page.fill('[data-testid="trace-target"]', "example.com")
    await page.click('[data-testid="trace-start"]')
    await expect(page.locator('[data-testid="trace-status"]')).toContainText(/completed/i)
    await page.screenshot({ path: shot(`reskin-traceroute-${theme}.png`), fullPage: true })
  })

  test(`reskin visual: web benchmark populated (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/download-speed")
    await setTheme(page, theme)
    await page.fill('[data-testid="download-url"]', "https://example.test/file.bin")
    await page.click('[data-testid="download-start"]')
    await expect(page.locator('[data-testid="download-results"]')).toBeVisible()
    await page.screenshot({ path: shot(`reskin-download-${theme}.png`), fullPage: true })
  })

  test(`reskin visual: lan scan populated (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/lan-scan")
    await setTheme(page, theme)
    await page.click('[data-testid="lan-start"]')
    await expect(page.locator('[data-testid="lan-status"]')).toContainText(/completed/i)
    await page.screenshot({ path: shot(`reskin-lan-${theme}.png`), fullPage: true })
  })

  test(`reskin visual: dns toolkit (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/dns-tester")
    await setTheme(page, theme)
    await page.waitForTimeout(500)
    await page.screenshot({ path: shot(`reskin-dns-${theme}.png`), fullPage: true })
  })

  test(`reskin visual: mikrotik live (${theme})`, async ({ page }) => {
    await installMockTauri(page)
    await page.goto("/#/mikrotik")
    await setTheme(page, theme)
    await expect(page.locator('[data-testid="mikrotik-view"]')).toBeVisible()
    await page.getByRole("button", { name: "Connect", exact: true }).click()
    await page.getByRole("tab", { name: "System" }).click()
    const systemPanel = page.getByRole("tabpanel", { name: "System" })
    await expect(systemPanel.locator('[data-testid="mikrotik-status-cpu"]')).toContainText("21%")
    await expect(systemPanel.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
    await page.screenshot({ path: shot(`reskin-mikrotik-${theme}.png`), fullPage: true })
  })
}
