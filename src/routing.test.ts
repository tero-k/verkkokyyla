import { describe, expect, it } from "vitest";
import { NAV_GROUPS, NAV_ITEMS, resolveRoute, type Route } from "./routing";

function groupOf(route: Route) {
  return NAV_GROUPS.find((group) => group.items.some((item) => item.route === route));
}

function shortcutOf(route: Route) {
  for (const group of NAV_GROUPS) {
    const item = group.items.find((entry) => entry.route === route);
    if (item !== undefined) return item.shortcut;
  }
  return undefined;
}

describe("app shell", () => {
  it("resolves each sidebar item's hash to its route", () => {
    for (const item of NAV_ITEMS) {
      expect(resolveRoute(item.hash)).toBe(item.route);
    }
  });

  it("keeps nine tool nav items with OPNsense between MikroTik and Help", () => {
    expect(NAV_ITEMS).toHaveLength(9);
    const order = NAV_ITEMS.map((item) => item.route);
    expect(order.indexOf("mikrotik")).toBeLessThan(order.indexOf("opnsense"));
    expect(order.indexOf("opnsense")).toBeLessThan(order.indexOf("help"));
  });

  it("groups MikroTik then OPNsense under Devices", () => {
    const devices = NAV_GROUPS.find((group) => group.label === "Devices");
    expect(devices?.items).toHaveLength(2);
    expect(devices?.items[0]?.route).toBe("mikrotik");
    expect(devices?.items[1]?.route).toBe("opnsense");
  });

  it("keeps every tool in exactly one shortcut group", () => {
    const routes = NAV_ITEMS.map((item) => item.route);
    for (const route of routes) {
      expect(groupOf(route), `group for ${route}`).toBeDefined();
    }
    const flat = NAV_GROUPS.flatMap((group) => group.items.map((item) => item.route));
    expect(new Set(flat).size).toBe(flat.length);
  });

  it("assigns Ctrl 7 to MikroTik, Ctrl 8 to OPNsense and Ctrl 9 to Help", () => {
    expect(shortcutOf("mikrotik")).toBe("Ctrl 7");
    expect(shortcutOf("opnsense")).toBe("Ctrl 8");
    expect(shortcutOf("help")).toBe("Ctrl 9");
  });

  it("numbers shortcuts sequentially across groups", () => {
    const flat = NAV_GROUPS.flatMap((group) => group.items);
    flat.forEach((item, index) => {
      expect(item.shortcut).toBe(`Ctrl ${index + 1}`);
    });
  });

  it("resolves the Ping hash to the ping route", () => {
    expect(resolveRoute("#/ping")).toBe("ping");
  });

  it("resolves the OPNsense hash to the opnsense route", () => {
    expect(resolveRoute("#/opnsense")).toBe("opnsense");
  });

  it("resolves the MikroTik hash to the mikrotik route", () => {
    expect(resolveRoute("#/mikrotik")).toBe("mikrotik");
  });

  it("resolves the Help hash to the help route", () => {
    expect(resolveRoute("#/help")).toBe("help");
  });

  it("resolves an unknown hash to the ping route", () => {
    expect(resolveRoute("#/nope")).toBe("ping");
  });

  it("resolves an empty hash to the ping route", () => {
    expect(resolveRoute("")).toBe("ping");
  });
});
