import { describe, expect, it } from "vitest";
import { NAV_ITEMS, resolveRoute } from "./routing";

describe("app shell", () => {
  it("has exactly one nav item labeled Ping", () => {
    expect(NAV_ITEMS).toHaveLength(1);
    expect(NAV_ITEMS[0]).toMatchObject({ route: "ping", label: "Ping" });
  });

  it("resolves the Ping hash to the ping route", () => {
    expect(resolveRoute("#/ping")).toBe("ping");
  });

  it("resolves an unknown hash to the ping route", () => {
    expect(resolveRoute("#/nope")).toBe("ping");
  });

  it("resolves an empty hash to the ping route", () => {
    expect(resolveRoute("")).toBe("ping");
  });
});
