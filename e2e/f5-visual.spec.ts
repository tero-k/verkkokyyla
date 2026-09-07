import { expect, test, type Page } from "@playwright/test"
import { mkdirSync } from "node:fs"
import path from "node:path"
import { installMockTauri } from "./mock-ipc"

test.use({ viewport: { width: 1024, height: 768 } })

const evidenceDir = path.join(process.cwd(), ".omo", "evidence")
type MikrotikTab = "Profiles" | "System" | "Interfaces" | "VLANs"

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
  await openTab(page, "System")
  const systemPanel = page.getByRole("tabpanel", { name: "System" })
  await expect(systemPanel.locator('[data-testid="mikrotik-status-cpu"]')).toContainText("21%")
  await expect(systemPanel.getByLabel("MikroTik versions")).toBeVisible()
  await expect(systemPanel.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(systemPanel.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await expect(systemPanel.locator('[data-testid="mikrotik-interface-graph"]')).toHaveCount(0)
  await openTab(page, "Interfaces")
  const interfacesPanel = page.getByRole("tabpanel", { name: "Interfaces" })
  await expect(interfacesPanel.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(interfacesPanel.locator('[data-testid="mikrotik-selected-interface"]')).toBeVisible()
  await expect(interfacesPanel.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await expect(interfacesPanel.locator('[data-testid="mikrotik-cpu-graph"]')).toHaveCount(0)
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
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-populated-live-monitoring.png")
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-interfaces-tab.png")
  await openTab(page, "VLANs")
  await captureFullPage(page, "f5-vlans-tab.png")
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
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-status-temperature"]')).toContainText("Not supported on this device")
  await expect(page.locator('[data-testid="mikrotik-status-fan"]')).toContainText("Not supported on this device")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("Not applicable")
  await expect(page.getByText("No saved MikroTik sessions yet.")).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-unsupported-empty-states.png")
})

test("F5 visual QA: profile setup landing tab", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  const profilesPanel = page.getByRole("tabpanel", { name: "Profiles" })
  await expect(profilesPanel.getByLabel("MikroTik profiles")).toBeVisible()
  await expect(profilesPanel.getByText("Backups use the selected profile.")).toBeVisible()
  await expect(profilesPanel.locator('[data-testid="mikrotik-backup-button"]')).toBeVisible()
  await expect(profilesPanel.getByLabel("MikroTik versions")).toHaveCount(0)
  await captureFullPage(page, "f5-profiles-tab.png")
})

test("F5 visual QA: responsive tabs stay inside the page", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)

  for (const width of [375, 768, 1280] as const) {
    await page.setViewportSize({ width, height: 768 })
    for (const name of ["Profiles", "System", "Interfaces", "VLANs"] as const) {
      await openTab(page, name)
      const panel = page.getByRole("tabpanel", { name })
      if (name === "System") {
        await expect(panel.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
      } else if (name === "Interfaces") {
        await expect(panel.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
      }
      const metrics = await panel.evaluate((element) => ({
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
      }))
      expect(metrics.scrollWidth, `${name} panel clips horizontally at ${width}px`).toBeLessThanOrEqual(metrics.clientWidth)
      await captureFullPage(page, `f5-responsive-${width}-${name.toLowerCase()}.png`)
    }

    const documentMetrics = await page.evaluate(() => ({
      clientWidth: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
    }))
    expect(documentMetrics.scrollWidth).toBe(documentMetrics.clientWidth)
  }
})

test("F5 visual QA: loaded historical session", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForLivePanels(page)
  await page.getByRole("button", { name: "Stop" }).click()
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(1)
  await page.locator('[data-testid="mikrotik-open-session"]').click()
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toContainText("3 snapshots")
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-interface-graph"] canvas')).toBeVisible()
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-cpu-graph"] canvas')).toBeVisible()
  await expect(page.locator('[data-testid="mikrotik-memory-graph"] canvas')).toBeVisible()
  await captureFullPage(page, "f5-loaded-historical-session.png")
})
