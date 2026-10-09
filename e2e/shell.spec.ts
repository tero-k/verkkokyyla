import { expect, test } from "@playwright/test";

test("sidebar shows all tool nav items and Devices shortcuts", async ({ page }) => {
  await page.goto("/");
  const navItems = page.locator("nav[aria-label='Tools'] a");
  await expect(navItems).toHaveCount(9);
  // Nav items include a "Ctrl N" kbd hint after the label.
  await expect(navItems.nth(0)).toHaveText(/Ping/);
  await expect(navItems.nth(1)).toHaveText(/Traceroute/);
  await expect(navItems.nth(2)).toHaveText(/Web Benchmark/);
  await expect(navItems.nth(3)).toHaveText(/Network scanner/);
  await expect(navItems.nth(4)).toHaveText(/MTU Discovery/);
  await expect(navItems.nth(5)).toHaveText(/DNS Toolkit/);
  await expect(navItems.nth(6)).toHaveText(/MikroTik.*Ctrl 7/);
  await expect(navItems.nth(7)).toHaveText(/OPNsense.*Ctrl 8/);
  await expect(navItems.nth(8)).toHaveText(/Help.*Ctrl 9/);
  await expect(page.locator("nav[aria-label='Tools']")).toContainText("Devices");
});

test("unknown hash renders the Ping view", async ({ page }) => {
  await page.goto("/#/nope");
  await expect(page.locator("h1")).toHaveText("Ping / ICMP");
  await expect(page.locator("nav[aria-label='Tools'] a").first()).toHaveText(
    /Ping/,
  );
});

test("download speed hash renders the download speed view", async ({ page }) => {
  await page.goto("/#/download-speed");
  await expect(page.getByTestId("download-speed-view")).toBeVisible();
});

test("help hash renders the help view with how-to sections", async ({ page }) => {
  await page.goto("/#/help");
  await expect(page.getByTestId("help-view")).toBeVisible();
  await expect(page.locator("h1")).toHaveText("Help & how to");
  await expect(page.getByTestId("help-view")).toContainText("MikroTik: terminal");
  await expect(page.getByTestId("help-view")).toContainText("MTU Discovery");
});
