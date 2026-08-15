import { describe, expect, it } from "vitest"
import { validateTarget } from "./validate"

describe("validateTarget", () => {
  it("accepts a plain hostname", () => {
    const result = validateTarget("example.com")
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.value).toBe("example.com")
  })

  it("accepts an IPv4 address", () => {
    const result = validateTarget("192.168.1.1")
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.value).toBe("192.168.1.1")
  })

  it("accepts the loopback IPv6 address", () => {
    const result = validateTarget("::1")
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.value).toBe("::1")
  })

  it("accepts an IPv6 address with a zone id", () => {
    const result = validateTarget("fe80::1%3")
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.value).toBe("fe80::1%3")
  })

  it("rejects an empty target", () => {
    const result = validateTarget("")
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error).toBe("target is required")
  })

  it("rejects an IPv4 address with out-of-range octets", () => {
    const result = validateTarget("999.1.1.1")
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error).toBe("invalid IPv4 address")
  })

  it("rejects a hostname containing a space", () => {
    const result = validateTarget("exa mple.com")
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error).toBe("target must not contain spaces")
  })
})
