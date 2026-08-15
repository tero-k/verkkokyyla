import { expect, test } from "@playwright/test";

test("sidebar shows exactly one Ping nav item", async ({ page }) => {
  await page.goto("/");
  const navItems = page.locator("nav[aria-label='Tools'] a");
  await expect(navItems).toHaveCount(1);
  await expect(navItems.first()).toHaveText("Ping");
});

test("unknown hash renders the Ping view", async ({ page }) => {
  await page.goto("/#/nope");
  await expect(page.locator("h1")).toHaveText("Ping");
  await expect(page.locator("nav[aria-label='Tools'] a").first()).toHaveText(
    "Ping",
  );
});
