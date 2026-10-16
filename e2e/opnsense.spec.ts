import { expect, test, type Locator, type Page } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

async function openOpnsense(page: Page): Promise<void> {
  await installMockTauri(page)
  await page.goto("/#/opnsense")
  await expect(page.getByTestId("opnsense-view")).toBeVisible()
  await expect(page.locator("#opnsense-profile")).toHaveValue("1")
}

async function tab(page: Page, name: string) {
  const button = page.getByRole("tab", { name, exact: true })
  await button.click()
  await expect(button).toHaveAttribute("aria-selected", "true")
  return page.getByRole("tabpanel", { name, exact: true })
}

async function connectMonitor(page: Page): Promise<void> {
  await page.getByTestId("opnsense-view").getByRole("button", { name: "Connect", exact: true }).first().click()
  await expect(page.getByTestId("opnsense-device-strip")).toBeVisible()
  await tab(page, "Firewall rules")
  await expect(page.getByTestId("opnsense-rule-row")).toHaveCount(2)
}

test("OPNsense sections follow the operational order", async ({ page }) => {
  await openOpnsense(page)
  await expect(page.getByRole("tablist", { name: "OPNsense sections" }).getByRole("tab")).toHaveText([
    "Profiles",
    "Overview",
    "Firewall rules",
    "Interfaces & VPN",
    "Firewall logs",
    "Firmware",
    "Terminal",
    "Backups",
  ])
})

test("Devices profile creation, API connection and packet hit counters", async ({ page }) => {
  await openOpnsense(page)
  await expect(page.locator('nav[aria-label="Tools"]')).toContainText("Devices")
  const profiles = await tab(page, "Profiles")
  await profiles.getByLabel("Profile name").fill("Branch firewall")
  await profiles.getByLabel("Host (DNS name or IP, no scheme)").fill("branch-fw.lab")
  await profiles.getByLabel("API key (read-only)").fill("readonly-key")
  await profiles.getByLabel("API secret").fill("readonly-secret")
  await profiles.getByRole("button", { name: "Save profile" }).click()
  await expect(page.getByTestId("opnsense-profile-3")).toContainText("API credentials stored")
  await page.getByTestId("opnsense-profile-3").getByRole("button", { name: "Test connection" }).click()
  await expect(page.getByTestId("opnsense-test-result")).toContainText("API key authorized")
  await page.locator("#opnsense-profile").selectOption("3")
  await connectMonitor(page)
  const rows = page.getByTestId("opnsense-rule-row")
  await expect(page.getByRole("table", { name: "Firewall rules — lan" }).getByRole("columnheader", { name: "Hits (packets)" })).toBeVisible()
  await expect(rows.filter({ hasText: "Permit Branch firewall" }).locator("td").nth(6)).toHaveText("12")
  await expect(rows.filter({ hasText: "Block Branch firewall" }).locator("td").nth(6)).toHaveText("0")
  await expect(page.getByTestId("opnsense-rules")).toContainText("older versions may omit legacy rules")
  const groups = page.getByTestId("opnsense-rule-group")
  await expect(groups).toHaveCount(2)
  await expect(groups.first()).toContainText("lan")
  await groups.first().click()
  await expect(groups.first()).toHaveAttribute("aria-expanded", "false")
  await expect(rows).toHaveCount(1)
  await groups.first().click()
  await expect(rows).toHaveCount(2)
})

test("read-only GET pagination limits are visible instead of duplicating rule or interface rows", async ({ page }) => {
  await openOpnsense(page)
  await page.evaluate(() => window.__TAURI_MOCK_SET_OPNSENSE_TRUNCATED__(1, true))
  await connectMonitor(page)
  const rules = page.getByTestId("opnsense-rules")
  await expect(rules.getByTestId("opnsense-rule-row")).toHaveCount(2)
  await expect(rules.getByRole("alert")).toContainText("showing 2 of 3")
  await expect(rules.getByRole("alert")).toContainText("read-only key cannot use POST")
  const vpn = await tab(page, "Interfaces & VPN")
  await expect(vpn.getByRole("alert")).toContainText("Interface list incomplete")
})

test("blocked and allowed firewall streams filter independently and search visible fields", async ({ page }) => {
  await openOpnsense(page)
  const logs = await tab(page, "Firewall logs")
  await logs.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  await logs.getByRole("group", { name: "Log action filter" }).getByText("Blocked").click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(1)
  await expect(logs.getByTestId("opnsense-log-row")).toContainText("203.0.113.18")
  await expect(logs.getByTestId("opnsense-log-row")).not.toContainText("192.0.2.23")
  await logs.getByRole("group", { name: "Log action filter" }).getByText("Allowed").click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(1)
  await expect(logs.getByTestId("opnsense-log-row")).toContainText("192.0.2.23")
  await logs.getByRole("searchbox", { name: "Filter log entries" }).fill("not-present")
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(0)
  await logs.getByRole("searchbox", { name: "Filter log entries" }).fill("198.51.100.8")
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(1)
  await logs.getByRole("button", { name: "Disconnect", exact: true }).click()
  await expect(logs.getByRole("button", { name: "Connect", exact: true })).toBeEnabled()
})

test("VPN sections survive missing Tailscale plugin and a healthy firewall reports peers", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 })
  await openOpnsense(page)
  await connectMonitor(page)
  let vpn = await tab(page, "Interfaces & VPN")
  await expect(vpn.getByRole("table", { name: "Network interfaces" })).toContainText("igb0")
  await expect(vpn.getByTestId("opnsense-vpn-wireguard")).toContainText("remote office")
  await expect(vpn.getByRole("table", { name: "Network interfaces" })).toContainText("aa:bb:cc:dd:ee:ff")
  await expect(vpn.getByRole("table", { name: "Network interfaces" })).toContainText("lagg0 · lacp")
  const wan = vpn.getByRole("table", { name: "Network interfaces" }).getByRole("row", { name: /igb0/ })
  await expect(wan).toContainText("4.0 KiB")
  await expect(wan).toContainText("2.0 KiB")
  await expect(wan).toContainText("123,456")
  await expect(wan).toContainText("78,910")
  await expect(vpn.getByRole("table", { name: "Network interfaces" })).toContainText("em0 (attached, active")
  await expect(vpn.getByTestId("opnsense-vpn-wireguard")).toContainText("203.0.113.2:51820")
  await expect(vpn.getByTestId("opnsense-vpn-openvpn")).toContainText("10.8.0.2")
  await expect(vpn.getByTestId("opnsense-vpn-wireguard")).toContainText("stale")
  await expect(vpn.getByTestId("opnsense-vpn-ipsec")).toContainText("branch tunnel")
  await expect(vpn.getByTestId("opnsense-vpn-ipsec")).toContainText("connected")
  await expect(vpn.getByTestId("opnsense-vpn-tailscale")).toContainText("Plugin unavailable")
  await page.locator("#opnsense-profile").selectOption("2")
  await connectMonitor(page)
  vpn = await tab(page, "Interfaces & VPN")
  await expect(vpn.getByTestId("opnsense-vpn-tailscale")).toContainText("workstation")
  await expect(vpn.getByTestId("opnsense-vpn-tailscale")).toContainText("100.101.102.103")
  const tailscale = vpn.getByTestId("opnsense-vpn-tailscale")
  const ownIps = tailscale.getByText(/fd7a:115c:a1e0::3e01:6f4f/)
  const offline = tailscale.getByText("offline", { exact: true })
  await expect(ownIps).toBeVisible()
  await expect(offline).toBeVisible()
  expect(await ownIps.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
  expect(await offline.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
  const peerName = tailscale.getByText("Tero's MacBook Pro")
  expect(await peerName.evaluate((el) => el.getBoundingClientRect().height)).toBeLessThan(25)
  await expect(vpn.getByTestId("opnsense-vpn-ipsec")).toContainText("connected")
})

test("wide OPNsense tables expose a usable horizontal control on narrow windows", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 650 })
  await openOpnsense(page)
  await connectMonitor(page)
  await expect(page.getByRole("tab", { name: "Terminal" })).toBeInViewport()

  async function scrollToLastColumn(panel: Locator, label: string, tableName: string, lastColumn: string) {
    const slider = panel.getByRole("slider", { name: `Scroll ${label} horizontally` })
    await expect(slider).toBeVisible()
    await slider.focus()
    await slider.press("End")
    await expect.poll(() => slider.evaluate((el) => {
      const scroller = el.previousElementSibling as HTMLElement
      return scroller.scrollLeft >= scroller.scrollWidth - scroller.clientWidth - 1
    })).toBe(true)
    const header = panel.getByRole("table", { name: tableName }).getByRole("columnheader", { name: lastColumn })
    expect(await header.evaluate((el) => {
      const viewport = el.closest("table")!.parentElement!.getBoundingClientRect()
      const cell = el.getBoundingClientRect()
      return cell.left >= viewport.left && cell.right <= viewport.right
    })).toBe(true)
  }

  let panel = await tab(page, "Firewall rules")
  await scrollToLastColumn(panel, "Firewall rules — lan", "Firewall rules — lan", "States")

  panel = await tab(page, "Firewall logs")
  await panel.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(panel.getByTestId("opnsense-log-row")).toHaveCount(2)
  await scrollToLastColumn(panel, "Firewall logs", "Firewall log entries", "Rule")

  panel = await tab(page, "Interfaces & VPN")
  await scrollToLastColumn(panel, "Network interfaces", "Network interfaces", "TX packets")
  // A real firewall can report enough interfaces to exceed the window height.
  // Simulate a long response without altering the default test fixtures.
  await panel.getByRole("table", { name: "Network interfaces" }).evaluate((table) => {
    const body = (table as HTMLTableElement).tBodies[0]
    for (let i = 0; i < 14; i++) body.appendChild(body.rows[0].cloneNode(true))
  })
  const bar = panel.getByRole("slider", { name: "Scroll Network interfaces horizontally" })
  expect(await bar.evaluate((el) => {
    const viewport = el.previousElementSibling as HTMLElement
    const bounds = el.closest('[role="tabpanel"]')!.getBoundingClientRect()
    return {
      tall: viewport.scrollHeight > viewport.clientHeight,
      reachable: el.getBoundingClientRect().bottom <= bounds.bottom,
    }
  })).toEqual({ tall: true, reachable: true })
  await page.setViewportSize({ width: 1800, height: 768 })
  await expect(panel.getByRole("slider", { name: "Scroll Network interfaces horizontally" })).toHaveCount(0)

  panel = await tab(page, "Backups")
  await expect(panel.getByRole("table", { name: "Saved configuration revisions" })).toBeVisible()
  await expect(panel.getByRole("slider", { name: "Scroll Saved configuration revisions horizontally" })).toHaveCount(0)
})

test("firmware compares cached releases and reports unknown when the cache is missing", async ({ page }) => {
  await openOpnsense(page)
  const firmware = await tab(page, "Firmware")
  await firmware.getByRole("button", { name: "Read cached status" }).click()
  await expect(firmware.getByTestId("opnsense-update-available")).toContainText("25.7.3 → 25.7.11")
  await expect(firmware.getByRole("heading", { name: "Newer releases" })).toBeVisible()
  await expect(firmware).toContainText("os-tailscale · 1.2")
  await expect(firmware).toContainText("cached upgrade → 1.3")
  await expect(firmware).toContainText("os-nginx · 1.35")
  await expect(firmware).toContainText("update availability unknown")
  await expect(firmware.getByTestId("opnsense-firmware-meta")).toContainText("ABI: 25.7")
  await page.locator("#opnsense-profile").selectOption("2")
  await firmware.getByRole("button", { name: "Read cached status" }).click()
  await expect(firmware.getByTestId("opnsense-firmware-unknown")).toContainText("No cached release changelog")
  await expect(firmware.getByTestId("opnsense-firmware-meta")).toContainText("Installed: 25.7.3")
  await expect(firmware.getByRole("heading", { name: "Newer releases" })).toHaveCount(0)
})

test("encrypted backup selects a saved revision, rejects weak passphrases and clears secrets", async ({ page }) => {
  await openOpnsense(page)
  const backups = await tab(page, "Backups")
  const revisions = backups.getByRole("table", { name: "Saved configuration revisions" })
  await expect(revisions).toContainText("config-20261004.xml")
  await expect(revisions).toContainText("config-20260930.xml")
  await backups.getByRole("button", { name: "Choose directory" }).click()
  await backups.getByLabel("Backup name").fill("edge-saved")
  await backups.getByLabel("Saved revision").selectOption("config-20260930.xml")
  const passphrase = backups.getByLabel("Passphrase (required, never stored)")
  await passphrase.fill("weak")
  await expect(backups.getByRole("button", { name: "Save encrypted backup" })).toBeDisabled()
  await passphrase.fill("correct horse battery staple")
  await backups.getByRole("button", { name: "Save encrypted backup" }).click()
  const result = backups.getByTestId("opnsense-backup-result")
  await expect(result).toContainText("edge-saved.xml.age")
  await expect(result).toContainText("config-20260930.xml")
  await expect(result).toContainText("2026-09-30T09:15:00Z")
  await expect(result).toContainText("age -d -o config.xml")
  await expect(passphrase).toHaveValue("")
  await expect(backups.getByRole("button", { name: "Save encrypted backup" })).toBeDisabled()
})

test("SSH requires independent login, explicit host trust, rejects bad user and echoes authenticated input", async ({ page }) => {
  await openOpnsense(page)
  const panel = await tab(page, "Terminal")
  const identity = panel.getByTestId("opnsense-terminal-identity")
  await expect(identity).toContainText("Edge firewall")
  await expect(identity).toContainText("edge-fw.lab:22")
  const credentials = panel.getByTestId("opnsense-terminal-credentials")
  await expect(credentials).toContainText("Separate OPNsense SSH login")
  await credentials.getByLabel("SSH username").fill("invalid")
  await credentials.getByLabel("SSH password").fill("wrong")
  await panel.getByRole("button", { name: "Connect", exact: true }).click()
  const trust = panel.getByTestId("opnsense-host-key-prompt")
  await expect(trust).toContainText("SHA-256:MOCK-EDGE-SHA256-KEY")
  await expect(panel.getByTestId("opnsense-terminal-empty")).toBeVisible()
  await expect(panel.getByRole("button", { name: "Disconnect", exact: true })).toBeDisabled()
  await trust.getByRole("button", { name: "Verify fingerprint & connect" }).click()
  await expect(panel.getByTestId("opnsense-terminal-credentials")).toContainText("SSH authentication failed")
  await expect(panel.getByTestId("opnsense-terminal-empty")).toBeVisible()
  await panel.getByLabel("SSH username").fill("ops")
  await panel.getByLabel("SSH password").fill("ssh-pass")
  await panel.getByRole("button", { name: "Connect", exact: true }).click()
  // A rejected login never pins the host key: verify again for the first
  // successful SSH authentication.
  await panel.getByTestId("opnsense-host-key-prompt").getByRole("button", { name: "Verify fingerprint & connect" }).click()
  const terminal = panel.getByTestId("opnsense-terminal-container")
  await expect(terminal.locator(".xterm")).toBeVisible()
  await expect(panel.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
  await expect(panel.getByLabel("SSH password")).toHaveCount(0)
  // xterm paints to a canvas rather than exposing shell text in the DOM.
  // Observe its actual painted surface change when the user types a command.
  const screen = terminal.locator(".xterm-screen")
  await expect.poll(async () => (await screen.screenshot()).byteLength).toBeGreaterThan(0)
  const before = await screen.screenshot()
  await terminal.locator(".xterm-helper-textarea").click()
  await page.keyboard.type("echo verified-shell")
  await page.keyboard.press("Enter")
  await expect.poll(async () => (await screen.screenshot()).equals(before)).toBe(false)
  await panel.getByRole("button", { name: "Disconnect", exact: true }).click()
  await expect(panel.getByTestId("opnsense-terminal-empty")).toBeVisible()
})

test("shortcuts, both themes and route keep-alive preserve active monitor, logs and shell", async ({ page }) => {
  await installMockTauri(page)
  await page.goto("/#/ping")
  await expect(page.getByRole("heading", { name: "Ping / ICMP" })).toBeVisible()
  await page.keyboard.press("Control+8")
  await expect(page).toHaveURL(/#\/opnsense$/)
  await expect(page.getByTestId("opnsense-view")).toBeVisible()
  await connectMonitor(page)
  const logs = await tab(page, "Firewall logs")
  await logs.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  for (const theme of ["Light", "Dark"]) {
    await page.getByRole("group", { name: "Theme" }).getByRole("button", { name: theme }).click()
    await expect(page.getByRole("group", { name: "Theme" }).getByRole("button", { name: theme })).toHaveAttribute("aria-pressed", "true")
    await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  }
  const terminalPanel = await tab(page, "Terminal")
  await terminalPanel.getByLabel("SSH username").fill("ops")
  await terminalPanel.getByLabel("SSH password").fill("ssh-pass")
  await terminalPanel.getByRole("button", { name: "Connect", exact: true }).click()
  await terminalPanel.getByTestId("opnsense-host-key-prompt").getByRole("button", { name: "Verify fingerprint & connect" }).click()
  await expect(terminalPanel.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
  await page.keyboard.press("Control+9")
  await expect(page.getByTestId("help-view")).toBeVisible()
  await page.keyboard.press("Control+7")
  await expect(page.getByTestId("mikrotik-view")).toBeVisible()
  await page.keyboard.press("Control+8")
  await expect(page.getByTestId("opnsense-view")).toBeVisible()
  await expect(page.getByTestId("opnsense-device-strip")).toBeVisible()
  await expect(terminalPanel.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
  await tab(page, "Firewall logs")
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  await expect(logs.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled()
  await logs.getByRole("button", { name: "Disconnect", exact: true }).click()
  await expect(logs.getByRole("button", { name: "Connect", exact: true })).toBeEnabled()
  await tab(page, "Terminal")
  await terminalPanel.getByRole("button", { name: "Disconnect", exact: true }).click()
  await expect(terminalPanel.getByTestId("opnsense-terminal-empty")).toBeVisible()
  await page.getByTestId("opnsense-view").getByRole("button", { name: "Disconnect", exact: true }).first().click()
  await expect(page.getByTestId("opnsense-device-strip")).toHaveCount(0)
})

test("profile switch never displays another firewall's rules or log buffer", async ({ page }) => {
  await openOpnsense(page)
  await connectMonitor(page)
  await expect(page.getByTestId("opnsense-rules")).toContainText("Permit Edge firewall")
  const logs = await tab(page, "Firewall logs")
  await logs.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  await expect(logs).toContainText("Edge firewall blocked ingress")
  await page.locator("#opnsense-profile").selectOption("2")
  await expect(logs).not.toContainText("Edge firewall blocked ingress")
  await logs.getByRole("button", { name: "Connect", exact: true }).click()
  await expect(logs.getByTestId("opnsense-log-row")).toHaveCount(2)
  await expect(logs).toContainText("Healthy firewall blocked ingress")
  await tab(page, "Firewall rules")
  await expect(page.getByTestId("opnsense-rules")).not.toContainText("Permit Edge firewall")
  await connectMonitor(page)
  await expect(page.getByTestId("opnsense-rules")).toContainText("Permit Healthy firewall")
  await page.locator("#opnsense-profile").selectOption("1")
  await expect(page.getByTestId("opnsense-rules")).toContainText("Permit Edge firewall")
  await expect(page.getByTestId("opnsense-rules")).not.toContainText("Permit Healthy firewall")
  await tab(page, "Firewall logs")
  await expect(logs).toContainText("Edge firewall blocked ingress")
  await expect(logs).not.toContainText("Healthy firewall blocked ingress")
})
