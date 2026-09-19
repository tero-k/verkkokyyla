import { describe, expect, it } from "vitest";
import { NAV_GROUPS, NAV_ITEMS, resolveRoute } from "./routing";

describe("app shell", () => {
  it("has the eight tool nav items in sidebar order", () => {
    expect(NAV_ITEMS).toHaveLength(8);
    expect(NAV_ITEMS).toEqual([
      { route: "ping", label: "Ping", hash: "#/ping" },
      { route: "traceroute", label: "Traceroute", hash: "#/traceroute" },
      {
        route: "download-speed",
        label: "Web Benchmark",
        hash: "#/download-speed",
      },
      {
        route: "lan-scan",
        label: "Network scanner",
        hash: "#/lan-scan",
      },
      { route: "mtu", label: "MTU Discovery", hash: "#/mtu" },
      {
        route: "dns-tester",
        label: "DNS Toolkit",
        hash: "#/dns-tester",
      },
      {
        route: "mikrotik",
        label: "MikroTik",
        hash: "#/mikrotik",
      },
      { route: "help", label: "Help", hash: "#/help" },
    ]);
  });

  it("assigns MTU Discovery to the Discover group at Ctrl 5", () => {
    expect(NAV_GROUPS).toEqual([
      {
        label: "Measure",
        items: [
          { route: "ping", shortcut: "Ctrl 1" },
          { route: "traceroute", shortcut: "Ctrl 2" },
          { route: "download-speed", shortcut: "Ctrl 3" },
        ],
      },
      {
        label: "Discover",
        items: [
          { route: "lan-scan", shortcut: "Ctrl 4" },
          { route: "mtu", shortcut: "Ctrl 5" },
          { route: "dns-tester", shortcut: "Ctrl 6" },
        ],
      },
      {
        label: "MikroTik",
        items: [{ route: "mikrotik", shortcut: "Ctrl 7" }],
      },
      {
        label: "Guide",
        items: [{ route: "help", shortcut: "Ctrl 8" }],
      },
    ]);
  });

  it("resolves the Ping hash to the ping route", () => {
    expect(resolveRoute("#/ping")).toBe("ping");
  });

  it("resolves the Web Benchmark hash to the download-speed route", () => {
    expect(resolveRoute("#/download-speed")).toBe("download-speed");
  });

  it("resolves the Traceroute hash to the traceroute route", () => {
    expect(resolveRoute("#/traceroute")).toBe("traceroute");
  });

  it("resolves the MTU Discovery hash to the mtu route", () => {
    expect(resolveRoute("#/mtu")).toBe("mtu");
  });

  it("resolves the Network scanner hash to the lan-scan route", () => {
    expect(resolveRoute("#/lan-scan")).toBe("lan-scan");
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
