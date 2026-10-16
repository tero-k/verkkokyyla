import { expect, test, type Locator, type Page } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

async function reachLastColumn(page: Page, label: string, table: Locator, column: string): Promise<void> {
  const slider = page.getByRole("slider", { name: `Scroll ${label} horizontally` })
  await expect(slider).toBeVisible()
  await slider.focus()
  await slider.press("End")
  await expect.poll(() => slider.evaluate((el) => {
    const viewport = el.previousElementSibling as HTMLElement
    return viewport.scrollLeft >= viewport.scrollWidth - viewport.clientWidth - 1
  })).toBe(true)
  const last = table.getByRole("columnheader", { name: column, exact: true })
  expect(await last.evaluate((el) => {
    const viewport = el.closest("table")!.parentElement!.getBoundingClientRect()
    const cell = el.getBoundingClientRect()
    return getComputedStyle(el).display !== "none" && cell.left >= viewport.left && cell.right <= viewport.right
  })).toBe(true)
}

test("narrow data tables reveal their last columns across device and diagnostic views", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 650 })
  await installMockTauri(page)
  await page.goto("/#/mikrotik")
  await page.getByTestId("mikrotik-view").getByRole("button", { name: "Connect", exact: true }).click()
  await page.getByRole("tab", { name: "Interfaces", exact: true }).click()
  const interfaces = page.getByRole("table", { name: "MikroTik interfaces" })
  await expect(interfaces.getByRole("row")).toHaveCount(3)
  await reachLastColumn(page, "MikroTik interfaces", interfaces, "TX collision")

  await page.goto("/#/lan-scan")
  await reachLastColumn(page, "Discovered network devices", page.getByRole("table", { name: "Discovered network devices" }), "Last seen")

  await page.goto("/#/dns-tester")
  await reachLastColumn(page, "DNS lookup records", page.locator("table").first(), "Data")
})
