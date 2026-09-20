import { describe, expect, it } from "vitest"
import { buildScanCsv, scanCsvFileName } from "./scanCsv"
import type { ScanHostDto } from "./types"

function makeHost(overrides: Partial<ScanHostDto> = {}): ScanHostDto {
  return {
    ip: "192.168.1.1",
    mac: "AA:BB:CC:DD:EE:01",
    vendor: "Router Corp",
    hostname: "gateway",
    foundBy: "arp",
    at: "2026-09-20T10:00:00.000Z",
    openPorts: [],
    ...overrides,
  }
}

describe("buildScanCsv", () => {
  it("starts with a UTF-8 BOM and the header row", () => {
    const csv = buildScanCsv([])
    expect(csv.charCodeAt(0)).toBe(0xfeff)
    expect(csv.slice(1)).toBe('"IP","MAC","Vendor","Hostname","Open ports","RTT","Last seen"\n')
  })

  it("quotes every cell and escapes embedded quotes", () => {
    const csv = buildScanCsv([makeHost({ vendor: 'ACME "Labs"' })])
    expect(csv).toContain('"ACME ""Labs"""')
  })

  it("renders open ports as space-separated port-service pairs", () => {
    const csv = buildScanCsv([
      makeHost({
        ip: "192.168.1.42",
        openPorts: [
          { port: 22, service: "ssh" },
          { port: 80, service: "http" },
        ],
      }),
    ])
    expect(csv).toContain('"22 ssh 80 http"')
  })

  it("emits empty cells for missing mac, vendor, and hostname", () => {
    const csv = buildScanCsv([makeHost({ mac: null, vendor: null, hostname: null })])
    const row = csv.split("\n")[1]
    expect(row).toBe('"192.168.1.1","","","","","","2026-09-20T10:00:00.000Z"')
  })
})

describe("scanCsvFileName", () => {
  it("replaces the CIDR slash with a dash", () => {
    expect(scanCsvFileName("192.168.1.10/24")).toBe("verkkokyyla-scan-192.168.1.10-24.csv")
  })
})
