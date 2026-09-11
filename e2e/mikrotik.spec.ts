import { expect, test, type Page } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

type MikrotikTab = "Profiles" | "Backups" | "System" | "Interfaces" | "VLANs"

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

async function startAndWaitForSnapshots(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Start" }).click()
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(page.locator('[data-testid="interface-row-sfp1"]')).toBeVisible()
}

async function stopAndLoadHistory(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Stop" }).click()
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(1)
  await page.locator('[data-testid="mikrotik-open-session"]').click()
}

test("navigation and profile lifecycle cover success and 401 test connection", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await expect(page.getByRole("heading", { name: "MikroTik monitoring" })).toBeVisible()

  await page.getByLabel("Profile name").fill("Branch router")
  await page.getByLabel("Host").fill("branch-router.lab")
  await page.getByLabel("Username").fill("ops")
  await page.getByLabel("Password", { exact: true }).fill("secret")
  await page.getByRole("button", { name: "Save profile" }).click()
  await expect(page.locator('[data-testid="profile-row-2"]')).toContainText("Branch router")

  await page.getByLabel("Test connection for Branch router").click()
  await expect(page.getByText(/Connection OK: RB5009 \/ RouterOS 7\.16/)).toBeVisible()

  await page.evaluate(() => window.__TAURI_MOCK_SET_MIKROTIK_TEST_401__(true))
  await page.getByLabel("Test connection for Branch router").click()
  await expect(page.getByText("MikroTik API returned 401 Unauthorized")).toBeVisible()
})

test("start populates panels, stop saves history, load restores data, and delete removes session", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await expect(page.getByRole("tabpanel", { name: "Profiles" }).getByLabel("MikroTik versions")).toHaveCount(0)
  await openTab(page, "System")
  await expect(page.locator('[data-testid="routeros-badge"]')).toHaveText("unknown")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("unknown")

  await startAndWaitForSnapshots(page)
  await openTab(page, "System")
  const systemPanel = page.getByRole("tabpanel", { name: "System" })
  await expect(systemPanel.locator('[data-testid="mikrotik-status-cpu"]')).toContainText("21%")
  await expect(systemPanel.locator('[data-testid="mikrotik-status-temperature"]')).toContainText("47")
  await expect(systemPanel.getByLabel("MikroTik versions")).toBeVisible()
  await expect(systemPanel.locator('[data-testid="mikrotik-interface-graph"]')).toHaveCount(0)
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="counter-rx-error"]').first()).toHaveText("3")
  await expect(page.locator('[data-testid="counter-rx-error"]').nth(1)).toHaveText("-")
  await openTab(page, "VLANs")
  await expect(page.locator('[data-testid="vlan-interface-row"]')).toContainText("vlan20-guests")
  await expect(page.locator('[data-testid="bridge-vlan-row"]')).toContainText("sfp1")
  await openTab(page, "System")
  await expect(page.locator('[data-testid="routeros-badge"]')).toHaveText("update available")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("upgrade available")

  await openTab(page, "Interfaces")
  await page.locator('[data-testid="interface-row-sfp1"]').click()
  const interfacesPanel = page.getByRole("tabpanel", { name: "Interfaces" })
  await expect(interfacesPanel.locator('[data-testid="mikrotik-selected-interface"]')).toHaveText("Selected interface: sfp1")
  await expect(interfacesPanel.getByRole("heading", { name: "Interface traffic: sfp1" })).toBeVisible()
  await expect(interfacesPanel.locator('[data-testid="mikrotik-interface-graph"]')).toHaveAttribute("data-interface", "sfp1")

  await stopAndLoadHistory(page)
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toContainText("RB5009 / 7.16")
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await openTab(page, "System")
  await expect(page.locator('[data-testid="routeros-badge"]')).toHaveText("update available")
  await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("upgrade available")
  await openTab(page, "Interfaces")
  await page.locator('[data-testid="interface-row-sfp1"]').click()
  await expect(page.locator('[data-testid="mikrotik-interface-graph"]')).toHaveAttribute("data-interface", "sfp1")

  await openTab(page, "System")
  await page.locator('[data-testid="mikrotik-delete-session"]').click()
  await page.locator('[data-testid="confirm-dialog-confirm"]').click()
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(0)
})

test("backup covers overwrite success, cancelled picker, and SSH unreachable error", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Backups")
  await expect(page.locator('[data-testid="mikrotik-backup-panel"]')).toBeVisible()
  await page.getByRole("button", { name: "Choose directory" }).click()
  await expect(page.getByText("C:/verkkokyyla-e2e/backups")).toBeVisible()
  await page.getByLabel("Backup name").fill("existing")
  await page.getByLabel("Include .rsc export").check()
  await page.getByRole("button", { name: "Create backup" }).click()
  await expect(page.locator('[data-testid="confirm-dialog"]')).toContainText("Overwrite")
  await page.locator('[data-testid="confirm-dialog-confirm"]').click()
  await expect(page.getByText("C:/verkkokyyla-e2e/backups/existing.backup")).toBeVisible()
  await expect(page.getByText("C:/verkkokyyla-e2e/backups/existing.rsc")).toBeVisible()

  await page.reload()
  await expect(page.locator('[data-testid="mikrotik-view"]')).toBeVisible()
  await openTab(page, "Backups")
  const backupCount = await page.evaluate(() => window.__TAURI_MOCK_MIKROTIK_BACKUP_COUNT__())
  await page.evaluate(() => window.__TAURI_MOCK_SET_DIALOG_CONFIRM__(false))
  await page.getByRole("button", { name: "Choose directory" }).click()
  await expect(page.getByRole("button", { name: "Create backup" })).toBeDisabled()
  await expect(page.evaluate(() => window.__TAURI_MOCK_MIKROTIK_BACKUP_COUNT__())).resolves.toBe(backupCount)

  await page.evaluate(() => {
    window.__TAURI_MOCK_SET_DIALOG_CONFIRM__(true)
    window.__TAURI_MOCK_SET_MIKROTIK_BACKUP_SSH_ERROR__(true)
  })
  await page.getByRole("button", { name: "Choose directory" }).click()
  await page.getByLabel("Backup name").fill("ssh-error")
  await page.getByRole("button", { name: "Create backup" }).click()
  await expect(page.getByText("SSH connection refused Enable the SSH service on the router (IP > Services)")).toBeVisible()
})

const versionCases = [
  { name: "available", variant: "available", router: true, routerBadge: "update available", firmwareBadge: "upgrade available" },
  { name: "up to date", variant: "up-to-date", router: true, routerBadge: "up to date", firmwareBadge: "up to date" },
  { name: "unknown", variant: "unknown", router: true, routerBadge: "unknown", firmwareBadge: "unknown" },
  { name: "not applicable firmware", variant: "na", router: false, routerBadge: "unknown", firmwareBadge: "Not applicable" },
]

for (const item of versionCases) {
  test(`version panel starts unknown and updates from emitted ${item.name} status`, async ({ page }) => {
    await openMikrotik(page)
    await openTab(page, "System")
    await page.evaluate(({ variant, router }) => {
      window.__TAURI_MOCK_SET_MIKROTIK_VERSION_VARIANT__(variant)
      window.__TAURI_MOCK_SET_MIKROTIK_ROUTERBOARD__(router)
    }, item)
    await expect(page.locator('[data-testid="routeros-badge"]')).toHaveText("unknown")
    await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText("unknown")
    await startAndWaitForSnapshots(page)
    await openTab(page, "System")
    await expect(page.locator('[data-testid="routeros-badge"]')).toHaveText(item.routerBadge)
    await expect(page.locator('[data-testid="firmware-badge"]')).toHaveText(item.firmwareBadge)
  })
}
