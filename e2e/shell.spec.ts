import { expect, test } from "@playwright/test";

test("sidebar shows Ping, Traceroute, and Download speed test nav items", async ({ page }) => {
  await page.goto("/");
  const navItems = page.locator("nav[aria-label='Tools'] a");
  await expect(navItems).toHaveCount(3);
  await expect(navItems.nth(0)).toHaveText("Ping");
  await expect(navItems.nth(1)).toHaveText("Traceroute");
  await expect(navItems.nth(2)).toHaveText("Web page speed test");
});

test("unknown hash renders the Ping view", async ({ page }) => {
  await page.goto("/#/nope");
  await expect(page.locator("h1")).toHaveText("Ping");
  await expect(page.locator("nav[aria-label='Tools'] a").first()).toHaveText(
    "Ping",
  );
});

test("download speed hash renders the download speed view", async ({ page }) => {
  await page.goto("/#/download-speed");
  await expect(page.getByTestId("download-speed-view")).toBeVisible();
});
