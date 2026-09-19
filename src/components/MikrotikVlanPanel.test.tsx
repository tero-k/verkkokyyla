// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"
import type { MikrotikBridgeVlanDto, MikrotikVlanDto } from "../lib/types"
import { MikrotikVlanPanel } from "./MikrotikVlanPanel"

const vlans: readonly MikrotikVlanDto[] = [
  {
    name: "staff-vlan",
    vlanId: 20,
    interface: "bridge-core",
    running: true,
    disabled: false,
  },
  {
    name: "guest-vlan",
    vlanId: 30,
    interface: "ether4",
    running: false,
    disabled: true,
  },
]

const bridgeVlans: readonly MikrotikBridgeVlanDto[] = [
  {
    bridge: "bridge-core",
    vlanIds: ["20", "30-31"],
    tagged: ["bridge-core", "sfp1"],
    untagged: ["ether2"],
    currentTagged: ["bridge-core", "sfp1"],
    currentUntagged: ["ether2"],
  },
]

afterEach(cleanup)

describe("MikrotikVlanPanel", () => {
  it("renders VLAN interface and bridge VLAN fixture rows", () => {
    render(<MikrotikVlanPanel vlans={vlans} bridgeVlans={bridgeVlans} />)

    const vlanTable = screen.getByRole("table", { name: "VLAN interfaces" })
    const bridgeTable = screen.getByRole("table", { name: "Bridge VLAN entries" })

    expect(within(vlanTable).getByText("staff-vlan")).toBeDefined()
    expect(within(vlanTable).getByText("20")).toBeDefined()
    expect(within(vlanTable).getByText("bridge-core")).toBeDefined()
    expect(within(vlanTable).getByText("Up").className).toMatch(/running/)
    expect(within(vlanTable).getByText("Disabled").className).toMatch(/disabled/)
    expect(within(bridgeTable).getByText("20, 30-31")).toBeDefined()
    expect(within(bridgeTable).getByText("bridge-core")).toBeDefined()
    expect(within(bridgeTable).getAllByText("bridge-core, sfp1")).toHaveLength(2)
    expect(within(bridgeTable).getAllByText("ether2")).toHaveLength(2)
  })

  it("renders muted empty-state copy for both empty VLAN lists", () => {
    render(<MikrotikVlanPanel vlans={[]} bridgeVlans={[]} />)

    const emptyStates = screen.getAllByText("No VLANs configured")

    expect(emptyStates).toHaveLength(2)
    for (const emptyState of emptyStates) {
      expect(emptyState.className).toMatch(/empty/)
    }
  })
})
