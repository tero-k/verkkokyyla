import type { ScanHostDto } from "./types"

function csvCell(value: string): string {
  return `"${value.replaceAll('"', '""')}"`
}

// Builds the CSV text for a scan export. Starts with a UTF-8 BOM so
// spreadsheet apps (Excel) read non-ASCII vendor and host names correctly.
export function buildScanCsv(rows: readonly ScanHostDto[]): string {
  const records = [
    ["IP", "MAC", "Vendor", "Hostname", "Open ports", "RTT", "Last seen"],
    ...rows.map((row) => [
      row.ip,
      row.mac ?? "",
      row.vendor ?? "",
      row.hostname ?? "",
      row.openPorts.map((port) => `${port.port} ${port.service}`).join(" "),
      "",
      row.at,
    ]),
  ]
  const body = records.map((record) => record.map(csvCell).join(",")).join("\n")
  return "\uFEFF" + `${body}\n`

}

export function scanCsvFileName(cidr: string): string {
  return `verkkokyyla-scan-${cidr.replace("/", "-")}.csv`
}
