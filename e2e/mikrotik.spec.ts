import { expect, test, type Page } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

type MikrotikTab = "Profiles" | "Backups" | "System" | "Interfaces" | "VLANs" | "Logs" | "Terminal"

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
  await page.getByRole("button", { name: "Connect", exact: true }).click()
  await openTab(page, "Interfaces")
  await expect(page.locator('[data-testid="interface-row-ether1"]')).toBeVisible()
  await expect(page.locator('[data-testid="interface-row-sfp1"]')).toBeVisible()
}

async function stopAndLoadHistory(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Disconnect", exact: true }).click()
  await openTab(page, "System")
  await expect(page.locator('[data-testid="mikrotik-session-item"]')).toHaveCount(1)
  await page.locator('[data-testid="mikrotik-open-session"]').click()
}

test("navigation and profile lifecycle cover success and 401 test connection", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await expect(page.getByRole("heading", { name: "MikroTik", exact: true })).toBeVisible()

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
  const backupPanel = page.locator('[data-testid="mikrotik-backup-panel"]')
  await expect(backupPanel.getByText("C:/verkkokyyla-e2e/backups/existing.backup")).toBeVisible()
  await expect(backupPanel.getByText("C:/verkkokyyla-e2e/backups/existing.rsc")).toBeVisible()

  const library = page.locator('[data-testid="mikrotik-backup-library"]')
  await expect(library).toBeVisible()
  const row = page.locator('[data-testid="mikrotik-backup-row-1"]')
  await expect(row).toContainText("existing")
  await expect(row).toContainText("Lab router")
  await expect(row).toContainText("+ .rsc")
  await page.getByRole("button", { name: "Delete existing" }).click()
  await page.locator('[data-testid="confirm-dialog-confirm"]').click()
  await expect(page.locator('[data-testid="mikrotik-backup-row-1"]')).toHaveCount(0)
  await expect(page.getByText("No MikroTik backups saved yet.")).toBeVisible()

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

test("backup library compares two .rsc exports and shows the config diff", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Backups")

  const createBackup = async (name: string) => {
    await page.getByLabel("Backup name").fill(name)
    await page.getByLabel("Include .rsc export").check()
    await page.getByRole("button", { name: "Create backup" }).click()
    await expect(page.locator(`[data-testid^="mikrotik-backup-row-"]`).filter({ hasText: name })).toBeVisible()
  }

  // Destination picker is mocked to confirm; the panel remembers it after the first pick.
  await page.getByRole("button", { name: "Choose directory" }).click()
  await expect(page.getByText("C:/verkkokyyla-e2e/backups")).toBeVisible()

  await createBackup("first")
  await createBackup("second")

  await expect(page.locator('[data-testid="mikrotik-backup-compare"]')).toBeDisabled()
  await page.getByLabel("Select first for comparison").check()
  await page.getByLabel("Select second for comparison").check()
  await page.locator('[data-testid="mikrotik-backup-compare"]').click()

  const diff = page.locator('[data-testid="mikrotik-backup-diff"]')
  await expect(diff).toBeVisible()
  await expect(diff).toContainText("Config diff: first → second")
  await expect(diff).toContainText("+2 −1")
  await expect(diff.locator('[data-testid="mikrotik-backup-diff-add"]')).toHaveCount(2)
  await expect(diff.locator('[data-testid="mikrotik-backup-diff-remove"]')).toHaveCount(1)
  await expect(diff).toContainText("add chain=forward action=accept")
  await page.screenshot({ path: ".omo/evidence/mikrotik-backup-diff.png", fullPage: true })
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

test("log stream tab streams entries, highlights severities, filters, and stops", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Logs")

  const logsPanel = page.getByRole("tabpanel", { name: "Logs" })
  await expect(logsPanel.getByTestId("mikrotik-logs-empty")).toHaveText("Connect to the stream to see router logs.")

  await logsPanel.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(logsPanel.getByTestId("mikrotik-log-row").first()).toBeVisible()
  // Default cadence is 2 s per entry — poll generously rather than racing it.
  await expect
    .poll(async () => logsPanel.getByTestId("mikrotik-log-row").count(), { timeout: 15_000 })
    .toBeGreaterThan(3)

  const errorRows = logsPanel.locator('[data-testid="mikrotik-log-row"][data-severity="error"]')
  await expect(errorRows.first()).toBeVisible({ timeout: 15_000 })

  // Severity filter narrows to errors only (the mock emits an error every
  // 5th entry, ~10 s apart at the 2 s cadence — allow time for two).
  await logsPanel.getByRole("button", { name: "Errors" }).click()
  await expect(logsPanel.getByTestId("mikrotik-log-row").first()).toHaveAttribute("data-severity", "error")
  await expect(logsPanel.getByTestId("mikrotik-log-row").nth(1)).toHaveAttribute("data-severity", "error", {
    timeout: 30_000,
  })

  // Free-text filter on top of the severity filter.
  await logsPanel.getByLabel("Filter log messages").fill("no-such-message")
  await expect(logsPanel.getByTestId("mikrotik-log-row")).toHaveCount(0)
  await logsPanel.getByLabel("Filter log messages").fill("")
  await logsPanel.getByRole("button", { name: "All" }).click()

  await logsPanel.getByRole("button", { name: "Disconnect", exact: true }).click()
  const countAtStop = await logsPanel.getByTestId("mikrotik-log-row").count()
  await page.waitForTimeout(1200)
  expect(await logsPanel.getByTestId("mikrotik-log-row").count()).toBe(countAtStop)
})

test("terminal tab opens an SSH session, echoes input, and disconnects", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Terminal")

  const terminalPanel = page.getByRole("tabpanel", { name: "Terminal" })
  await expect(terminalPanel.getByTestId("mikrotik-terminal-identity")).toContainText("Lab router")
  await terminalPanel.getByRole("button", { name: "Connect", exact: true }).click()

  // xterm mounts its surface under the container. Output is canvas-rendered,
  // so assert structure (surface mounted, session live), not text.
  const terminal = terminalPanel.getByTestId("mikrotik-terminal-container")
  await expect(terminal.locator(".xterm")).toBeVisible()
  const disconnect = terminalPanel.getByRole("button", { name: "Disconnect", exact: true })
  await expect(disconnect).toBeEnabled()

  // Typing into the xterm helper textarea drives a backend write; the mock
  // echoes the chunk back without disturbing the session.
  await terminal.locator(".xterm-helper-textarea").click()
  await page.keyboard.type("interface print")
  await page.keyboard.press("Enter")
  await expect(disconnect).toBeEnabled()

  await disconnect.click()
  await expect(terminalPanel.getByRole("button", { name: "Connect", exact: true })).toBeEnabled()
})

test("terminals persist per device with a visible identity tag", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await page.getByLabel("Profile name").fill("Branch router")
  await page.getByLabel("Host").fill("branch-router.lab")
  await page.getByLabel("Username").fill("ops")
  await page.getByLabel("Password", { exact: true }).fill("secret")
  await page.getByRole("button", { name: "Save profile" }).click()
  await expect(page.locator('[data-testid="profile-row-2"]')).toContainText("Branch router")

  await openTab(page, "Terminal")
  const terminalPanel = page.getByRole("tabpanel", { name: "Terminal" })
  const labSlot = terminalPanel.getByTestId("mikrotik-terminal-slot-1")
  await expect(labSlot.getByTestId("mikrotik-terminal-identity")).toContainText("Lab router")
  await expect(labSlot.getByTestId("mikrotik-terminal-identity")).toContainText("router.lab")

  await labSlot.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()

  // Switch devices: Lab's shell hides but keeps running in the background.
  await page.locator("#mikrotik-profile").selectOption("2")
  const branchSlot = terminalPanel.getByTestId("mikrotik-terminal-slot-2")
  await expect(branchSlot.getByTestId("mikrotik-terminal-identity")).toContainText("Branch router")
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeHidden()

  // Back to Lab: the SAME terminal, still connected — no reconnect needed.
  await page.locator("#mikrotik-profile").selectOption("1")
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()
  await expect(labSlot.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
})

test("switching terminals warns with the connection name and can be silenced", async ({ page }) => {
  await openMikrotik(page)
  await openTab(page, "Profiles")
  await page.getByLabel("Profile name").fill("Branch router")
  await page.getByLabel("Host").fill("branch-router.lab")
  await page.getByLabel("Username").fill("ops")
  await page.getByLabel("Password", { exact: true }).fill("secret")
  await page.getByRole("button", { name: "Save profile" }).click()
  await expect(page.locator('[data-testid="profile-row-2"]')).toContainText("Branch router")

  await openTab(page, "Terminal")
  const terminalPanel = page.getByRole("tabpanel", { name: "Terminal" })
  const labSlot = terminalPanel.getByTestId("mikrotik-terminal-slot-1")
  await labSlot.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()

  // Branch has no terminal yet — switching to it does not warn.
  await page.locator("#mikrotik-profile").selectOption("2")
  await expect(page.getByTestId("terminal-switch-warning")).toHaveCount(0)

  // Open Branch's terminal, then switch back — the warning names Lab router.
  const branchSlot = terminalPanel.getByTestId("mikrotik-terminal-slot-2")
  await branchSlot.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(branchSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()
  await page.locator("#mikrotik-profile").selectOption("1")
  const warning = page.getByTestId("terminal-switch-warning")
  await expect(warning).toBeVisible()
  await expect(warning).toContainText("Lab router")
  await expect(warning).toContainText("router.lab")

  // Dismissed without silencing — the next switch warns again.
  await warning.getByTestId("terminal-switch-warning-dismiss").click()
  await expect(page.getByTestId("terminal-switch-warning")).toHaveCount(0)
  await page.locator("#mikrotik-profile").selectOption("2")
  await expect(page.getByTestId("terminal-switch-warning")).toContainText("Branch router")

  // Silenced for the session — switching again raises nothing.
  await page.getByLabel("Don't warn again this session").check()
  await page.getByTestId("terminal-switch-warning-dismiss").click()
  await page.locator("#mikrotik-profile").selectOption("1")
  await expect(labSlot.getByTestId("mikrotik-terminal-identity")).toBeVisible()
  await expect(page.getByTestId("terminal-switch-warning")).toHaveCount(0)
})

test("sessions keep collecting while the user works in other tools", async ({ page }) => {
  await openMikrotik(page)
  await startAndWaitForSnapshots(page)

  // Open a terminal too — it must survive navigation as well.
  await openTab(page, "Terminal")
  const terminalPanel = page.getByRole("tabpanel", { name: "Terminal" })
  const labSlot = terminalPanel.getByTestId("mikrotik-terminal-slot-1")
  await labSlot.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()

  // Work in Measure for a bit.
  await page.locator('a[href="#/ping"]').click()
  await expect(page.locator('[data-testid="mikrotik-view"]')).toBeHidden()
  await page.waitForTimeout(800)

  // Back: still attached (no detached chip, no reconnect banner) and the
  // terminal is still live.
  await page.locator('a[href="#/mikrotik"]').click()
  await expect(page.locator('[data-testid="mikrotik-view"]')).toBeVisible()
  const chip = page.getByTestId("mikrotik-device-1")
  await expect(chip).toBeVisible()
  await expect(chip.getByText("detached")).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Reconnect" })).toHaveCount(0)
  await expect(labSlot.getByTestId("mikrotik-terminal-container").locator(".xterm")).toBeVisible()
  await expect(labSlot.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
})
