import { expect, test } from "@playwright/test";

// Placeholder: no browser fixture used, so this passes without downloaded
// browsers. Real UI specs land in later todos.
test("scaffold placeholder", () => {
  expect("verkkokyyla").toContain("kyyla");
});
