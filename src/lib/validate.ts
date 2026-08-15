import type { ValidationResult } from "./types"

function isBlank(s: string): boolean {
  return s.length === 0
}

function containsWhitespace(s: string): boolean {
  return /\s/.test(s)
}

function isValidIPv4Octet(octet: string): boolean {
  if (octet.length === 0 || octet.length > 3) return false
  if (octet.length > 1 && octet.startsWith("0")) return false
  const value = Number(octet)
  return Number.isInteger(value) && value >= 0 && value <= 255
}

function isValidIPv4(input: string): boolean {
  if (!/^\d{1,3}(\.\d{1,3}){3}$/.test(input)) return false
  return input.split(".").every(isValidIPv4Octet)
}

function isValidIPv6(input: string): boolean {
  const zoneIndex = input.indexOf("%")
  const addressPart = zoneIndex >= 0 ? input.slice(0, zoneIndex) : input
  const zonePart = zoneIndex >= 0 ? input.slice(zoneIndex + 1) : ""

  if (zoneIndex >= 0 && zonePart.length === 0) return false
  if (!/^[0-9a-fA-F:]*$/.test(addressPart)) return false

  const parts = addressPart.split(":")
  if (parts.length < 3) return false

  const emptyRuns = parts.reduce((runs, part, index) => {
    if (part.length !== 0) return runs
    if (index === 0 || parts[index - 1].length !== 0) return runs + 1
    return runs
  }, 0)
  if (emptyRuns > 1) return false
  if (emptyRuns === 0 && parts.length !== 8) return false
  if (parts.length > 9) return false

  for (const part of parts) {
    if (part.length === 0) continue
    if (part.length > 4) return false
    if (Number.isNaN(Number.parseInt(part, 16))) return false
  }

  return true
}

function isValidHostname(input: string): boolean {
  if (/^\d/.test(input)) return false
  if (/[:%]/.test(input)) return false
  if (!/^[a-zA-Z0-9][a-zA-Z0-9-._]*$/.test(input)) return false
  if (input.endsWith(".") || input.endsWith("-")) return false
  return true
}

export function validateTarget(input: string): ValidationResult {
  const trimmed = input.trim()

  if (isBlank(trimmed)) {
    return { ok: false, error: "target is required" }
  }

  if (containsWhitespace(trimmed)) {
    return { ok: false, error: "target must not contain spaces" }
  }

  if (trimmed.includes(":")) {
    if (isValidIPv6(trimmed)) {
      return { ok: true, value: trimmed }
    }
    return { ok: false, error: "invalid IPv6 address" }
  }

  if (/^\d/.test(trimmed)) {
    if (isValidIPv4(trimmed)) {
      return { ok: true, value: trimmed }
    }
    return { ok: false, error: "invalid IPv4 address" }
  }

  if (isValidHostname(trimmed)) {
    return { ok: true, value: trimmed }
  }

  return { ok: false, error: "invalid hostname or address" }
}
