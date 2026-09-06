import { describe, expect, it } from "vitest";
import { NAV_ITEMS, resolveRoute } from "./routing";

describe("app shell", () => {
  it("has Ping, Traceroute, Web Benchmark, Network scanner, DNS Toolkit, and MikroTik nav items", () => {
    expect(NAV_ITEMS).toHaveLength(6);
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

  it("resolves the Network scanner hash to the lan-scan route", () => {
    expect(resolveRoute("#/lan-scan")).toBe("lan-scan");
  });

  it("resolves the MikroTik hash to the mikrotik route", () => {
    expect(resolveRoute("#/mikrotik")).toBe("mikrotik");
  });

  it("resolves an unknown hash to the ping route", () => {
    expect(resolveRoute("#/nope")).toBe("ping");
  });

  it("resolves an empty hash to the ping route", () => {
    expect(resolveRoute("")).toBe("ping");
  });
});
