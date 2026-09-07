import { expect, test, type Page } from "@playwright/test"
import { mkdirSync } from "node:fs"
import path from "node:path"
import { installMockTauri } from "./mock-ipc"

test.use({ viewport: { width: 1024, height: 768 } })

const evidenceDir = path.join(process.cwd(), ".omo", "evidence")
type MikrotikTab = "Profiles" | "Statistics" | "Interfaces" | "VLANs"

function evidencePath(fileName: string): string {
  mkdirSync(evidenceDir, { recursive: true })
  return path.join(evidenceDir, fileName)
}

async function openMikrotik(page: Page): Promise<void> {
  await installMockTauri(page)
  await page.goto("/#/mikrotik")
  await expect(page.locator('[data-testid="mikrotik-view"]')).toBeVisible()
}

async function openTab(page: Page, name: MikrotikTab): Promise<void> {
  const tab = page.getByRole("tab", { name })
  await tab.click()
  await expect(tab).toHaveAttribute("aria-selected", "true")
}

async function startAndWaitForLivePanels(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Start" }).click()
  await openTab(page, "Statistics")
  await expect(page.locator('[data-testid="mikrotik-status-cpu"]')).toContainText("21%")
  await expect(page.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await openTab(page, "VLANs")
  await expect(page.locator('[data-testid="vlan-interface-row"]')).toContainText("vlan20-guests")
  await expect(page.locator('[data-testid="bridge-vlan-row"]')).toContainText("sfp1")
}

async function captureFullPage(page: Page, fileName: string): Promise<void> {
  await page.screenshot({ path: evidencePath(fileName), fullPage: true })
}

test("F5 visual QA: populated live monitoring", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)
  await openTab(page, "Statistics")
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
  await openTab(page, "VLANs")
  await expect(page.getByText("No VLANs configured")).toHaveCount(2)
  await page.evaluate(() => {
    window.__TAURI_MOCK_SET_MIKROTIK_ROUTERBOARD__(false)
    window.__TAURI_MOCK_SET_MIKROTIK_VERSION_VARIANT__("na")
  })
  await page.getByRole("button", { name: "Start" }).click()
  await openTab(page, "Statistics")
  await expect(page.locator('[data-testid="mikrotik-status-temperature"]')).toContainText("Not supported on this device")
  await expect(page.locator('[data-testid="mikrotik-status-fan"]')).toContainText("Not supported on this device")
  await openTab(page, "Profiles")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("Not applicable")
  await openTab(page, "Statistics")
  await expect(page.getByText("No saved MikroTik sessions yet.")).toBeVisible()
  await captureFullPage(page, "f5-unsupported-empty-states.png")
})

test("F5 visual QA: profile setup landing tab", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await expect(page.getByLabel("MikroTik profiles")).toBeVisible()
  await expect(page.getByLabel("MikroTik versions")).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-backup-button"]')).toBeVisible()
  await captureFullPage(page, "f5-profiles-tab.png")
})

test("F5 visual QA: narrow viewport keeps tab content inside the page", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 768 })
  await openMikrotik(page)

  for (const name of ["Profiles", "Statistics", "Interfaces", "VLANs"] as const) {
    await openTab(page, name)
    const panel = page.getByRole("tabpanel", { name })
    const metrics = await panel.evaluate((element) => ({
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
    }))
    expect(metrics.scrollWidth, `${name} panel clips horizontally`).toBeLessThanOrEqual(metrics.clientWidth)
  }

  const documentMetrics = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }))
  expect(documentMetrics.scrollWidth).toBe(documentMetrics.clientWidth)
})

test("F5 visual QA: loaded historical session", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)
  await page.getByRole("button", { name: "Stop" }).click()
  await openTab(page, "Statistics")
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(1)
  await page.locator('[data-testid="mikrotik-open-session"]').click()
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toContainText("3 snapshots")
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await openTab(page, "Statistics")
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-loaded-historical-session.png")
})
