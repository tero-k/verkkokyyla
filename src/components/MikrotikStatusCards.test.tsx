// @vitest-environment jsdom
import { cleanup, render, within } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"
import type {
  MikrotikResourcesDto,
  MikrotikSensorDto,
  MikrotikSessionSummaryDto,
  MikrotikSnapshotEvent,
  MikrotikTestConnectionDto,
} from "../lib/types"
import { MikrotikStatusCards } from "./MikrotikStatusCards"

const resources: MikrotikResourcesDto = {
  cpuLoad: 27,
  memUsedBytes: 1_073_741_824,
  memTotalBytes: 2_147_483_648,
  uptime: "3d 04:12:33",
  boardName: "CCR2216-live",
  routerosVersion: "7.20-live",
  architectureName: "arm64-live",
}

const sessionMetadata: MikrotikSessionSummaryDto = {
  id: 22,
  profileId: 7,
  startedAt: "2026-09-06T12:00:00Z",
  endedAt: null,
  status: "running",
  boardName: "CCR2004-1G-12S+2XS",
  routerosVersion: "7.19.4",
  architectureName: "arm64",
  updateStatusJson: null,
  firmwareStatusJson: null,
  snapshotCount: 3,
}

function snapshot(
  sensors: readonly MikrotikSensorDto[] | null,
  snapshotResources: MikrotikResourcesDto | null = resources,
): MikrotikSnapshotEvent {
  return {
    event: "snapshot",
    sessionId: 22,
    at: "2026-09-06T12:00:10Z",
    resources: snapshotResources,
    sensors,
    sensorsSupported: sensors !== null,
    interfaces: [],
    vlans: null,
    bridgeVlans: null,
    warning: null,
  }
}

afterEach(cleanup)

describe("MikrotikStatusCards", () => {
  it("renders resource values and live resource device metadata", () => {
    const view = render(
      <MikrotikStatusCards snapshot={snapshot([])} metadata={sessionMetadata} />,
    )

    expect(within(view.getByTestId("mikrotik-status-cpu")).getByText("27%")).toBeTruthy()
    expect(
      within(view.getByTestId("mikrotik-status-memory")).getByText("1 GiB / 2 GiB"),
    ).toBeTruthy()
    expect(view.getByText("3d 04:12:33")).toBeTruthy()
    expect(view.getByText("CCR2216-live")).toBeTruthy()
    // RouterOS version is intentionally not duplicated here; it lives in the
    // version panel with the update check.
    expect(view.queryByText("ROUTEROS VERSION")).toBeNull()
    expect(view.getByText("ARCHITECTURE")).toBeTruthy()
    expect(view.getByText("arm64-live")).toBeTruthy()
  })

  it("falls back to session device metadata when live resources omit identity", () => {
    const view = render(
      <MikrotikStatusCards
        snapshot={snapshot([], { ...resources, boardName: null, routerosVersion: null, architectureName: null })}
        metadata={sessionMetadata}
      />,
    )

    expect(view.getByText("CCR2004-1G-12S+2XS")).toBeTruthy()
    expect(view.getByText("arm64")).toBeTruthy()
  })

  it("renders temperature, fan-speed, and voltage-number aliases", () => {
    const sensors: readonly MikrotikSensorDto[] = [
      { name: "unrelated", value: 999, unit: null, kind: "other" },
      { name: "cpu-temp", value: 47.5, unit: "°C", kind: "temperature" },
      { name: "fan2-speed", value: 2_450, unit: "RPM", kind: "fan" },
      { name: "voltage3", value: 24.2, unit: "V", kind: "voltage" },
    ]

    const view = render(
      <MikrotikStatusCards snapshot={snapshot(sensors)} metadata={sessionMetadata} />,
    )

    expect(within(view.getByTestId("mikrotik-status-temperature")).getByText("47.5°C")).toBeTruthy()
    expect(within(view.getByTestId("mikrotik-status-fan")).getByText("2450 RPM")).toBeTruthy()
    expect(within(view.getByTestId("mikrotik-status-voltage")).getByText("24.2V")).toBeTruthy()
  })

  it("uses an SFP temperature when it is the only temperature sensor", () => {
    const view = render(
      <MikrotikStatusCards
        snapshot={snapshot([
          { name: "sfp-temperature", value: 39.5, unit: "°C", kind: "temperature" },
        ])}
        metadata={sessionMetadata}
      />,
    )

    expect(within(view.getByTestId("mikrotik-status-temperature")).getByText("39.5°C")).toBeTruthy()
  })

  it("skips a zero voltage rail in favor of the first meaningful rail", () => {
    const view = render(
      <MikrotikStatusCards
        snapshot={snapshot([
          { name: "psu1-voltage", value: 0, unit: "V", kind: "voltage" },
          { name: "psu2-voltage", value: 12.1, unit: "V", kind: "voltage" },
        ])}
        metadata={sessionMetadata}
      />,
    )

    expect(within(view.getByTestId("mikrotik-status-voltage")).getByText("12.1V")).toBeTruthy()
  })

  it("renders connection-sample architecture metadata", () => {
    const metadata: MikrotikTestConnectionDto = {
      boardName: "RB5009UG+S+",
      routerosVersion: "7.18.2",
      architectureName: "arm64",
    }

    const view = render(
      <MikrotikStatusCards
        snapshot={snapshot([], { ...resources, boardName: null, routerosVersion: null, architectureName: null })}
        metadata={metadata}
      />,
    )

    expect(within(view.getByTestId("mikrotik-status-architecture")).getByText("arm64")).toBeTruthy()
  })

  it("renders unsupported sensor states without zero or NaN placeholders", () => {
    const view = render(
      <MikrotikStatusCards snapshot={snapshot([], null)} metadata={null} />,
    )

    expect(view.getAllByText("Not supported on this device")).toHaveLength(3)
    const sensorText = ["temperature", "fan", "voltage"]
      .map((kind) => view.getByTestId(`mikrotik-status-${kind}`).textContent)
      .join(" ")
    expect(sensorText).not.toContain("NaN")
    expect(sensorText).not.toMatch(/\b0(?:\.0+)?(?:°C| RPM|V)\b/)
  })
})
