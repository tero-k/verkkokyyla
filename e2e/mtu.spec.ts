import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

test.beforeEach(async ({ page }) => {
  await installMockTauri(page)
})

test("sidebar entry and Ctrl+5 navigate to MTU Discovery", async ({ page }) => {
  await page.goto("/")

  await expect(page.getByRole("link", { name: /MTU Discovery/ })).toBeVisible()
  await page.keyboard.press("Control+5")

  await expect(page).toHaveURL(/#\/mtu$/)
  await expect(page.getByTestId("mtu-view")).toBeVisible()
})

test("full run renders streamed probes and the exact MTU result", async ({ page }) => {
  await page.goto("/#/mtu")

  await page.getByTestId("mtu-target").fill("example.com")
  await page.getByTestId("mtu-start").click()

  await expect(page.getByTestId("mtu-result-value")).toHaveText("1420")
  const rows = page.getByTestId("mtu-probe-row")
  await expect(rows).toHaveCount(10)
  await expect(rows.filter({ hasText: "1500" })).toContainText("too-big")
  await expect(rows.filter({ hasText: "1420" }).first()).toContainText("ok")
})

test("TCP method reveals its port control", async ({ page }) => {
  await page.goto("/#/mtu")

  await expect(page.getByTestId("mtu-port")).toHaveCount(0)
  await page.getByRole("button", { name: "TCP", exact: true }).click()
  await expect(page.getByTestId("mtu-port")).toBeVisible()
  await expect(page.getByTestId("mtu-port")).toBeEnabled()
  await expect(page.getByTestId("mtu-port")).toHaveValue("443")

  await page.getByRole("button", { name: "ICMP", exact: true }).click()
  await expect(page.getByTestId("mtu-port")).toHaveCount(0)
})

test("saved history run can be loaded into the probe table", async ({ page }) => {
  await page.goto("/#/mtu")

  const historyRows = page.getByTestId("mtu-history-row")
  await expect(historyRows).toHaveCount(2)
  const tcpRun = historyRows.filter({ hasText: "edge.example" })
  await tcpRun.getByTestId("mtu-history-load").click()

  await expect(page.getByTestId("mtu-result-value")).toHaveText("≥1500")
  await expect(page.getByTestId("mtu-probe-row")).toHaveCount(2)
  await expect(page.getByTestId("mtu-probe-table")).toContainText("1600")
})

test("stop cancels an active probe run", async ({ page }) => {
  await page.goto("/#/mtu")

  await page.getByTestId("mtu-target").fill("cancel.example")
  await page.getByTestId("mtu-start").click()
  await expect(page.getByTestId("mtu-stop")).toBeEnabled()
  await page.getByTestId("mtu-stop").click()

  await expect(page.getByTestId("mtu-status")).toContainText(/cancelled/i)
  await expect(page.getByTestId("mtu-start")).toBeEnabled()
})
