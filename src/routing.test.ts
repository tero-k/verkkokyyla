import { describe, expect, it } from "vitest";
import { NAV_ITEMS, resolveRoute } from "./routing";

describe("app shell", () => {
  it("has Ping and Download speed test nav items", () => {
    expect(NAV_ITEMS).toHaveLength(2);
    expect(NAV_ITEMS).toEqual([
      { route: "ping", label: "Ping", hash: "#/ping" },
      {
        route: "download-speed",
        label: "Download speed test",
        hash: "#/download-speed",
      },
    ]);
  });

  it("resolves the Ping hash to the ping route", () => {
    expect(resolveRoute("#/ping")).toBe("ping");
  });

  it("resolves the Download speed hash to the download-speed route", () => {
    expect(resolveRoute("#/download-speed")).toBe("download-speed");
  });

  it("resolves an unknown hash to the ping route", () => {
    expect(resolveRoute("#/nope")).toBe("ping");
  });

  it("resolves an empty hash to the ping route", () => {
    expect(resolveRoute("")).toBe("ping");
  });
});
