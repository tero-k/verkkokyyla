// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MikrotikInterfaceDto } from "../lib/types"
import { MikrotikInterfaceTable } from "./MikrotikInterfaceTable"

const limitedInterface: MikrotikInterfaceDto = {
  name: "ether-limited",
  type: "ether",
  running: false,
  disabled: false,
  rxByte: null,
  txByte: null,
  rxPacket: null,
  txPacket: null,
  txQueueDrop: null,
  linkDowns: null,
  rxError: null,
  txError: null,
  rxDrop: null,
  rxErrorEvents: null,
  txErrorEvents: null,
  rxFcsError: null,
  rxAlignError: null,
  txCollision: null,
  txDrop: null,
  rate: null,
  fullDuplex: null,
  rxBitsPerSecond: null,
  txBitsPerSecond: null,
}

const monitoredInterface: MikrotikInterfaceDto = {
  ...limitedInterface,
  name: "ether1",
  running: true,
  rxByte: 1_048_576,
  txByte: 1_024,
  rxPacket: 4_000,
  txPacket: 5_000,
  rate: "1Gbps",
  fullDuplex: true,
  rxBitsPerSecond: 1_500_000,
  txBitsPerSecond: 750,
}

const erroringInterface: MikrotikInterfaceDto = {
  ...monitoredInterface,
  name: "ether-errors",
  rxFcsError: 4,
  txDrop: 2,
}

const errorCounterFields = [
  "tx-queue-drop",
  "tx-drop",
  "link-downs",
  "rx-error",
  "tx-error",
  "rx-drop",
  "rx-error-events",
  "tx-error-events",
  "rx-fcs-error",
  "tx-collision",
] as const

afterEach(cleanup)

describe("MikrotikInterfaceTable", () => {
  it("renders unavailable error counters as dashes when the driver omits them", () => {
    render(
      <MikrotikInterfaceTable
        interfaces={[limitedInterface]}
        onSelectInterface={() => undefined}
      />,
    )

    const row = screen.getByTestId("interface-row-ether-limited")

    for (const field of errorCounterFields) {
      expect(within(row).getByTestId(`counter-${field}`).textContent).toBe("-")
    }
  })

  it("highlights positive rx-fcs-error and tx-drop counters", () => {
    render(
      <MikrotikInterfaceTable
        interfaces={[erroringInterface]}
        onSelectInterface={() => undefined}
      />,
    )

    const row = screen.getByTestId("interface-row-ether-errors")
    const rxFcsError = within(row).getByTestId("counter-rx-fcs-error")
    const txDrop = within(row).getByTestId("counter-tx-drop")

    expect(rxFcsError.textContent).toBe("4")
    expect(rxFcsError.className).toMatch(/warning/)
    expect(txDrop.textContent).toBe("2")
    expect(txDrop.className).toMatch(/warning/)
  })

  it("renders Ethernet monitor data and humanized traffic values when available", () => {
    const nonEthernet: MikrotikInterfaceDto = {
      ...limitedInterface,
      name: "vlan20",
      type: "vlan",
      rate: "10Gbps",
      fullDuplex: true,
    }
    render(
      <MikrotikInterfaceTable
        interfaces={[monitoredInterface, limitedInterface, nonEthernet]}
        onSelectInterface={() => undefined}
      />,
    )

    const monitoredRow = screen.getByTestId("interface-row-ether1")
    const limitedRow = screen.getByTestId("interface-row-ether-limited")
    const nonEthernetRow = screen.getByTestId("interface-row-vlan20")

    expect(within(monitoredRow).getByTestId("link").textContent).toBe(
      "1Gbps · full duplex",
    )
    expect(within(monitoredRow).getByTestId("rx-rate").textContent).toBe(
      "1.50 Mbit/s",
    )
    expect(within(monitoredRow).getByTestId("tx-rate").textContent).toBe(
      "750 bit/s",
    )
    expect(within(monitoredRow).getByTestId("rx-bytes").textContent).toBe("1.00 MB")
    expect(within(monitoredRow).getByTestId("tx-bytes").textContent).toBe("1.00 KB")
    expect(within(limitedRow).getByTestId("link").textContent).toBe("-")
    expect(within(nonEthernetRow).getByTestId("link").textContent).toBe("-")
  })

  it("renders running, disabled, and down status badges", () => {
    const disabledInterface: MikrotikInterfaceDto = {
      ...limitedInterface,
      name: "ether-disabled",
      running: true,
      disabled: true,
    }
    render(
      <MikrotikInterfaceTable
        interfaces={[monitoredInterface, disabledInterface, limitedInterface]}
        onSelectInterface={() => undefined}
      />,
    )

    expect(screen.getByText("Running").className).toMatch(/running/)
    expect(screen.getByText("Disabled").className).toMatch(/disabled/)
    expect(screen.getByText("Down").className).toMatch(/down/)
  })

  it("selects an interface when its row is clicked", () => {
    const onSelectInterface = vi.fn<(name: string) => void>()
    render(
      <MikrotikInterfaceTable
        interfaces={[monitoredInterface]}
        onSelectInterface={onSelectInterface}
      />,
    )

    fireEvent.click(screen.getByTestId("interface-row-ether1"))

    expect(onSelectInterface).toHaveBeenCalledOnce()
    expect(onSelectInterface).toHaveBeenCalledWith("ether1")
  })
})
