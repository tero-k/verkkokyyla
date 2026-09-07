import { expect, test, type Page } from "@playwright/test"
import { mkdirSync } from "node:fs"
import path from "node:path"
import { installMockTauri } from "./mock-ipc"

test.use({ viewport: { width: 1024, height: 768 } })

const evidenceDir = path.join(process.cwd(), ".omo", "evidence")

function evidencePath(fileName: string): string {
  mkdirSync(evidenceDir, { recursive: true })
  return path.join(evidenceDir, fileName)
}

async function openMikrotik(page: Page): Promise<void> {
  await installMockTauri(page)
  await page.goto("/#/mikrotik")
  await expect(page.locator('[data-testid="mikrotik-view"]')).toBeVisible()
}

async function startAndWaitForLivePanels(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Start" }).click()
  await expect(page.locator('[data-testid="mikrotik-status-cpu"]')).toContainText("21%")
  await expect(page.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(page.locator('[data-testid="vlan-interface-row"]')).toContainText("vlan20-guests")
  await expect(page.locator('[data-testid="bridge-vlan-row"]')).toContainText("sfp1")
}

async function captureFullPage(page: Page, fileName: string): Promise<void> {
  await page.screenshot({ path: evidencePath(fileName), fullPage: true })
}

test("F5 visual QA: populated live monitoring", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)
  await captureFullPage(page, "f5-populated-live-monitoring.png")
})

test("F5 visual QA: unsupported sensors and empty states", async ({ page }) => {
  await page.addInitScript(() => {
    const originalSetTimeout = window.setTimeout.bind(window)
    window.setTimeout = ((handler: TimerHandler, timeout?: number, ...args: unknown[]) => {
      const delay = timeout === 45 ? 5_000 : timeout
      return originalSetTimeout(handler, delay, ...args)
    }) as typeof window.setTimeout
  })
  await openMikrotik(page)
  await page.evaluate(() => {
    window.__TAURI_MOCK_SET_MIKROTIK_ROUTERBOARD__(false)
    window.__TAURI_MOCK_SET_MIKROTIK_VERSION_VARIANT__("na")
  })
  await page.getByRole("button", { name: "Start" }).click()
  await expect(page.locator('[data-testid="mikrotik-status-temperature"]')).toContainText("Not supported on this device")
  await expect(page.locator('[data-testid="mikrotik-status-fan"]')).toContainText("Not supported on this device")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("Not applicable")
  await page.locator('[data-testid="mikrotik-vlan-panel"]').scrollIntoViewIfNeeded()
  await captureFullPage(page, "f5-unsupported-empty-states.png")
})

test("F5 visual QA: backup dialog", async ({ page }) => {
  await openMikrotik(page)
  await page.locator('[data-testid="mikrotik-backup-button"]').click()
  await expect(page.getByRole("dialog", { name: "Create MikroTik backup" })).toBeVisible()
  await page.getByRole("button", { name: "Choose directory" }).click()
  await expect(page.getByText("C:/verkkokyyla-e2e/backups")).toBeVisible()
  await page.getByLabel("Backup name").fill("nightly-visual-check")
  await page.getByLabel("Include .rsc export").check()
  await captureFullPage(page, "f5-backup-dialog.png")
})

test("F5 visual QA: loaded historical session", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)
  await page.getByRole("button", { name: "Stop" }).click()
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(1)
  await page.locator('[data-testid="mikrotik-open-session"]').click()
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toContainText("3 snapshots")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-loaded-historical-session.png")
})
