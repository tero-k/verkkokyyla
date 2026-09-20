import { expect, test } from "@playwright/test"
import { installMockTauri } from "./mock-ipc"

const RELEASE = {
  version: "9.9.9",
  url: "https://github.com/tero-k/verkkokyyla/releases/tag/v9.9.9",
  current: "0.1.3",
}

/** Point the mock's update check at a fixed release before the app loads. */
async function mockRelease(
  page: import("@playwright/test").Page,
  result: typeof RELEASE,
): Promise<void> {
  await page.addInitScript((r) => {
    window.__TAURI_MOCK_SET_UPDATE_CHECK_RESULT__(r)
  }, result)
}

test("shows a banner with a release link when a newer version exists", async ({
  page,
}) => {
  await installMockTauri(page)
  await mockRelease(page, RELEASE)
  await page.goto("/")

  const banner = page.getByTestId("update-banner")
  await expect(banner).toBeVisible()
  await expect(banner).toContainText("v9.9.9")
  await expect(banner).toContainText("v0.1.3")

  await banner.getByRole("button", { name: "View release" }).click()
  const opened = await page.evaluate(() => window.__TAURI_MOCK_LAST_OPENED_URL__())
  expect(opened).toBe(RELEASE.url)
})

test("dismiss hides the banner until a newer version appears", async ({ page }) => {
  await installMockTauri(page)
  await mockRelease(page, RELEASE)
  await page.goto("/")

  const banner = page.getByTestId("update-banner")
  await expect(banner).toBeVisible()
  await banner.getByRole("button", { name: "Dismiss update notice" }).click()
  await expect(banner).toHaveCount(0)

  // Same version after a restart: still hidden (dismissal is per-version).
  await page.reload()
  await expect(banner).toHaveCount(0)

  // A strictly newer version brings the notice back.
  await mockRelease(page, { ...RELEASE, version: "9.9.10" })
  await page.reload()
  await expect(banner).toBeVisible()
  await expect(banner).toContainText("v9.9.10")
})

test("opting out in Help stops the startup check entirely", async ({ page }) => {
  await installMockTauri(page)
  await mockRelease(page, RELEASE)
  await page.goto("/")

  const banner = page.getByTestId("update-banner")
  await expect(banner).toBeVisible()

  await page.goto("/#/help")
  await page.getByTestId("update-check-toggle").uncheck()

  await page.reload()
  await expect(banner).toHaveCount(0)
  // The mock counter resets per page load, so the proof is that the check
  // window elapsed with zero invocations: opting out means no request is
  // made at all.
  await page.waitForTimeout(2500)
  const count = await page.evaluate(() => window.__TAURI_MOCK_UPDATE_CHECK_COUNT__())
  expect(count).toBe(0)
})
