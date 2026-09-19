import { expect, test, type Page } from "@playwright/test"
import { mkdirSync } from "node:fs"
import path from "node:path"
import { installMockTauri } from "./mock-ipc"

test.use({ viewport: { width: 1440, height: 1000 } })

// Captures drift per run (mock data uses wall-clock timestamps), so the spec
// writes into the tracked docs/screenshots/ only when explicitly regenerating:
//   UPDATE_README_SHOTS=1 npx playwright test e2e/readme-shots.spec.ts
// Regular runs capture into gitignored test-results/ instead.
const screenshotsDir = process.env.UPDATE_README_SHOTS
  ? path.join(process.cwd(), "docs", "screenshots")
  : path.join(process.cwd(), "test-results", "readme-shots")

type Theme = "light" | "dark"
type MikrotikTab = "System" | "Logs"

function screenshotPath(fileName: string): string {
  mkdirSync(screenshotsDir, { recursive: true })
  return path.join(screenshotsDir, fileName)
}

async function openMockedView(
  page: Page,
  route: string,
  theme: Theme,
): Promise<void> {
  await installMockTauri(page)
  await page.addInitScript((selectedTheme) => {
    localStorage.setItem("verkkokyyla-theme", selectedTheme)
  }, theme)
  await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" })
  await page.goto(route)
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme)
}

async function settleAndCapture(page: Page, fileName: string): Promise<void> {
  await page.evaluate(async () => {
    await document.fonts.ready
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
    window.scrollTo(0, 0)
  })
  await page.waitForTimeout(250)
  await page.screenshot({
    path: screenshotPath(fileName),
    animations: "disabled",
    caret: "hide",
    scale: "css",
  })
}

function pingProbes(offset = 0) {
  const now = Date.now()
  return Array.from({ length: 48 }, (_, index) => {
    const seq = index + 1
    const lost = seq % 17 === 0
    return {
      seq,
      rttMs: lost ? null : 4.2 + ((seq * 11 + offset) % 29) / 2.8,
      lost,
      at: new Date(now - (48 - index) * 1_000).toISOString(),
    }
  })
}

async function openMikrotikTab(page: Page, name: MikrotikTab): Promise<void> {
  const tab = page.getByRole("tab", { name })
  await tab.click()
  await expect(tab).toHaveAttribute("aria-selected", "true")
}

async function populateMikrotikMonitoring(page: Page): Promise<void> {
  const connect = page.getByRole("button", { name: "Connect", exact: true })
  await connect.click()
  await openMikrotikTab(page, "System")
  const systemPanel = page.getByRole("tabpanel", { name: "System" })
  await expect(systemPanel.getByTestId("mikrotik-status-cpu")).toContainText("21%")

  await page.getByRole("button", { name: "Disconnect", exact: true }).click()
  await expect(systemPanel.getByTestId("mikrotik-session-item")).toHaveCount(1)

  await connect.click()
  await expect(systemPanel.getByTestId("mikrotik-status-cpu")).toContainText("21%")
  await expect(systemPanel.getByTestId("mikrotik-cpu-graph").locator("canvas")).toBeVisible()
  await expect(systemPanel.getByTestId("mikrotik-memory-graph").locator("canvas")).toBeVisible()
}

async function installDnsReadmeFixtures(page: Page): Promise<void> {
  await page.evaluate(() => {
    const internals = window.__TAURI_INTERNALS__ as unknown as {
      invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
    }
    const originalInvoke = internals.invoke.bind(internals)

    internals.invoke = async (command, args = {}) => {
      if (command === "dns_lookup") {
        const recordTypes = args.recordTypes as string[]
        const onEvent = args.onEvent as { onmessage?: (message: unknown) => void }
        const answers: Record<string, readonly { data: string; ttl: number }[]> = {
          A: [
            { data: "93.184.216.34", ttl: 300 },
            { data: "93.184.216.35", ttl: 300 },
          ],
          AAAA: [{ data: "2606:2800:220:1:248:1893:25c8:1946", ttl: 300 }],
          MX: [{ data: "10 mail.example.com.", ttl: 1800 }],
          NS: [
            { data: "a.iana-servers.net.", ttl: 86400 },
            { data: "b.iana-servers.net.", ttl: 86400 },
          ],
          SOA: [{ data: "ns.icann.org. noc.dns.icann.org. 2026091901 7200 3600 1209600 3600", ttl: 3600 }],
          TXT: [{ data: "v=spf1 include:_spf.example.com -all", ttl: 1800 }],
        }
        const latencies = [7.8, 10.5, 13.2, 15.9, 18.6, 21.3]
        recordTypes.forEach((recordType, index) => {
          onEvent.onmessage?.({
            queryName: "example.com",
            recordType,
            result: {
              queryName: "example.com",
              recordType,
              rcode: "NOERROR",
              answers: answers[recordType.toUpperCase()] ?? [],
              authorityNodata: false,
              authoritySoa: recordType.toUpperCase() === "SOA",
              adFlag: true,
              aaFlag: false,
              raFlag: true,
              truncated: false,
              ednsPresent: true,
              latencyMs: latencies[index] ?? 24,
              transportUsed: "udp",
              responseBytes: 128 + index * 37,
              additionalGlue: [],
            },
          })
        })
        return {
          name: "example.com",
          resolver: "1.1.1.1",
          completed: recordTypes.length,
          failed: 0,
          elapsedMs: 31,
        }
      }

      if (command === "dns_diagnostics") {
        return {
          domain: "example.com",
          durationMs: 184,
          overallStatus: "pass",
          summary: "Resolution, delegation, DNSSEC, and authoritative answers are healthy.",
          results: [
            {
              id: "recursive-resolution",
              title: "Recursive resolution",
              status: "pass",
              summary: "Cloudflare returned NOERROR with complete A, AAAA, MX, and TXT data.",
              evidence: [
                { label: "Resolver", value: "Cloudflare · 1.1.1.1 · UDP" },
                { label: "Response", value: "NOERROR · 18.4 ms · EDNS0" },
              ],
            },
            {
              id: "delegation",
              title: "Delegation chain",
              status: "pass",
              summary: "Parent and child nameserver sets agree; glue addresses are reachable.",
              evidence: [
                { label: "Parent NS", value: "a.iana-servers.net, b.iana-servers.net" },
                { label: "SOA serial", value: "2026091901 · consistent on 2/2 authorities" },
              ],
            },
            {
              id: "dnssec",
              title: "DNSSEC validation",
              status: "pass",
              summary: "The resolver authenticated the answer chain from the root trust anchor.",
              evidence: [
                { label: "Validation", value: "AD flag set · DO bit requested" },
                { label: "Transport", value: "UDP response complete · no TCP retry" },
              ],
            },
          ],
          inventory: [
            { recordType: "A", status: "present", count: 2 },
            { recordType: "AAAA", status: "present", count: 1 },
            { recordType: "MX", status: "present", count: 1 },
            { recordType: "TXT", status: "present", count: 3 },
          ],
          technicalDetails: null,
        }
      }

      return originalInvoke(command, args)
    }
  })
}

async function installMikrotikLogReadmeFixture(page: Page): Promise<void> {
  await page.evaluate(() => {
    const internals = window.__TAURI_INTERNALS__ as unknown as {
      invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
    }
    const originalInvoke = internals.invoke.bind(internals)
    const entries = [
      { id: "*201", time: "12:47:03", topics: ["system", "info"], message: "router rebooted after scheduled maintenance", severity: "info" },
      { id: "*202", time: "12:47:08", topics: ["interface", "info"], message: "ether1 link up (speed 1G, full duplex)", severity: "info" },
      { id: "*203", time: "12:47:11", topics: ["bridge", "info"], message: "bridge port ether1 entered forwarding state", severity: "info" },
      { id: "*204", time: "12:47:29", topics: ["dhcp", "info"], message: "dhcp1 assigned 192.168.88.142 to 5C:A6:E6:41:2D:90", severity: "info" },
      { id: "*205", time: "12:48:02", topics: ["system", "warning"], message: "clock adjusted by NTP: offset -1.24 seconds", severity: "warning" },
      { id: "*206", time: "12:48:19", topics: ["account", "info"], message: "user admin logged in from 192.168.88.10 via winbox", severity: "info" },
      { id: "*207", time: "12:48:47", topics: ["firewall", "info"], message: "input accepted: in:ether1 proto UDP dst-port 51820", severity: "info" },
      { id: "*208", time: "12:49:06", topics: ["wireguard", "info"], message: "peer branch-office handshake completed", severity: "info" },
      { id: "*209", time: "12:49:22", topics: ["dns", "info"], message: "cache size 612 KiB, 184 active entries", severity: "info" },
      { id: "*210", time: "12:49:51", topics: ["system", "error"], message: "login failure for user api from 192.168.88.77 via ssh", severity: "error" },
      { id: "*211", time: "12:50:13", topics: ["interface", "info"], message: "sfp1 optical signal -4.2 dBm", severity: "info" },
      { id: "*212", time: "12:50:36", topics: ["dhcp", "warning"], message: "dhcp1 lease pool has 14 addresses remaining", severity: "warning" },
      { id: "*213", time: "12:50:58", topics: ["route", "info"], message: "default route via 203.0.113.1 is active", severity: "info" },
      { id: "*214", time: "12:51:17", topics: ["system", "info"], message: "RouterOS package update check completed", severity: "info" },
      { id: "*215", time: "12:51:34", topics: ["firewall", "error"], message: "input dropped: invalid connection state from ether1", severity: "error" },
      { id: "*216", time: "12:51:48", topics: ["wireless", "info"], message: "guest-wifi client connected, signal -58 dBm", severity: "info" },
      { id: "*217", time: "12:52:05", topics: ["system", "info"], message: "configuration autosave completed", severity: "info" },
      { id: "*218", time: "12:52:24", topics: ["interface", "info"], message: "ether1 traffic 2.4 Mbit/s rx, 1.5 Mbit/s tx", severity: "info" },
    ]

    internals.invoke = async (command, args = {}) => {
      if (command === "mikrotik_log_start") {
        const profileId = Number(args.profileId)
        const onEvent = args.onEvent as { onmessage?: (message: unknown) => void }
        const onStatus = args.onStatus as { onmessage?: (message: unknown) => void }
        window.setTimeout(() => {
          onStatus.onmessage?.({ event: "started", profileId })
          onEvent.onmessage?.({ event: "entries", entries })
        }, 20)
        return { profileId }
      }
      if (command === "mikrotik_log_stop") return null
      return originalInvoke(command, args)
    }
  })
}

test("README: populated ping in dark theme", async ({ page }) => {
  await openMockedView(page, "/#/ping", "dark")
  await page.getByTestId("ping-target").fill("core-gateway.example.net")
  await page.getByTestId("ping-start").click()
  await expect(page.getByTestId("resolved-ip")).toHaveText("192.0.2.1")

  await page.evaluate((events) => window.__TAURI_MOCK_SEND_PROBES__(events), pingProbes())
  await expect(page.getByTestId("aggregates-bar")).toContainText("48")
  await page.getByTestId("ping-stop").click()
  await expect(page.getByTestId("session-item")).toContainText("core-gateway.example.net")

  await page.getByTestId("ping-start").click()
  await page.evaluate((events) => window.__TAURI_MOCK_SEND_PROBES__(events), pingProbes(9))
  await expect(page.getByTestId("aggregates-bar")).toContainText("48")
  await expect(page.getByRole("img", { name: "Round-trip time chart" }).locator("canvas")).toBeVisible()
  await settleAndCapture(page, "ping-dark.png")
})

test("README: DNS evidence chain in light theme", async ({ page }) => {
  await openMockedView(page, "/#/dns-tester", "light")
  await installDnsReadmeFixtures(page)
  await page.getByRole("tab", { name: "Diagnostics" }).click()
  await page.getByRole("button", { name: "Run diagnostics" }).click()
  await expect(page.getByText("Delegation chain")).toBeVisible()
  await expect(page.getByText("DNSSEC validation")).toBeVisible()
  await expect(page.getByText(/root trust anchor/)).toBeVisible()
  await settleAndCapture(page, "dns-diagnostics-light.png")
})

test("README: DNS resolver race in dark theme", async ({ page }) => {
  await openMockedView(page, "/#/dns-tester", "dark")
  await installDnsReadmeFixtures(page)
  await page.getByRole("button", { name: "Resolve" }).click()
  await expect(page.getByText(/NOERROR · 8 records · 31 ms/)).toBeVisible()
  await expect(page.getByText("DNSSEC validated")).toBeVisible()
  await expect(page.getByText(/6 of 6 record types completed/)).toBeVisible()
  await settleAndCapture(page, "dns-lookup-dark.png")
})

test("README: MikroTik monitoring in dark theme", async ({ page }) => {
  await openMockedView(page, "/#/mikrotik", "dark")
  await expect(page.getByTestId("mikrotik-view")).toBeVisible()
  await populateMikrotikMonitoring(page)
  await expect(page.getByLabel("MikroTik versions")).toBeVisible()
  await expect(page.getByTestId("mikrotik-session-item")).toContainText("3 snapshots")
  await settleAndCapture(page, "mikrotik-monitoring-dark.png")
})

test("README: MikroTik logs in dark theme", async ({ page }) => {
  await openMockedView(page, "/#/mikrotik", "dark")
  await expect(page.getByTestId("mikrotik-view")).toBeVisible()
  await openMikrotikTab(page, "Logs")
  await installMikrotikLogReadmeFixture(page)
  const logsPanel = page.getByRole("tabpanel", { name: "Logs" })
  await logsPanel.getByRole("button", { name: "Connect", exact: true }).click()
  await expect
    .poll(() => logsPanel.getByTestId("mikrotik-log-row").count())
    .toBeGreaterThanOrEqual(18)
  await expect(logsPanel.locator('[data-severity="error"]').first()).toBeVisible()
  await settleAndCapture(page, "mikrotik-logs-dark.png")
})
