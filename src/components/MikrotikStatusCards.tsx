import { formatMetric } from "../lib/format"
import type {
  MikrotikResourcesDto,
  MikrotikSensorDto,
  MikrotikSessionSummaryDto,
  MikrotikSnapshotEvent,
  MikrotikTestConnectionDto,
} from "../lib/types"

import styles from "./MikrotikStatusCards.module.css"

const BYTE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const
const UNSUPPORTED = "Not supported on this device"

type MikrotikDeviceMetadata =
  | MikrotikSessionSummaryDto
  | MikrotikTestConnectionDto
  | MikrotikResourcesDto

export type MikrotikStatusCardsProps = {
  readonly snapshot: MikrotikSnapshotEvent | null
  readonly metadata: MikrotikDeviceMetadata | null
}

type StatusCard = {
  readonly id: string
  readonly label: string
  readonly value: string
  readonly highlighted: boolean
  readonly unsupported: boolean
}

function compactMetric(value: number, digits = 2): string {
  return String(Number(formatMetric(value, digits)))
}

function formatBytes(value: number | null): string {
  if (value === null || !Number.isFinite(value) || value < 0) return "-"

  let scaled = value
  let unitIndex = 0
  while (scaled >= 1_024 && unitIndex < BYTE_UNITS.length - 1) {
    scaled /= 1_024
    unitIndex += 1
  }

  return `${compactMetric(scaled)} ${BYTE_UNITS[unitIndex]}`
}

function formatCpu(value: number | null): string {
  return value !== null && Number.isFinite(value)
    ? `${compactMetric(value)}%`
    : "-"
}

function normalizedName(sensor: MikrotikSensorDto): string {
  return sensor.name.trim().toLowerCase()
}

function isTemperature(sensor: MikrotikSensorDto): boolean {
  return normalizedName(sensor).includes("temp")
}

function isFan(sensor: MikrotikSensorDto): boolean {
  return normalizedName(sensor).startsWith("fan")
}

function isVoltage(sensor: MikrotikSensorDto): boolean {
  const name = normalizedName(sensor)
  return (
    name.includes("voltage") ||
    name === "3.3v" ||
    name === "5v" ||
    name === "12v" ||
    name === "core"
  )
}

function firstFiniteSensor(
  sensors: readonly MikrotikSensorDto[],
  matches: (sensor: MikrotikSensorDto) => boolean,
): MikrotikSensorDto | null {
  return sensors.find((sensor) => matches(sensor) && Number.isFinite(sensor.value)) ?? null
}

function sensorValue(
  sensor: MikrotikSensorDto | null,
  suffix: string,
  spaced = false,
): string {
  if (sensor === null) return UNSUPPORTED
  return `${compactMetric(sensor.value)}${spaced ? " " : ""}${suffix}`
}

export function MikrotikStatusCards({
  snapshot,
  metadata,
}: MikrotikStatusCardsProps) {
  const resources = snapshot?.resources ?? null
  const sensors = snapshot?.sensors ?? []
  const temperature = firstFiniteSensor(sensors, isTemperature)
  const fan = firstFiniteSensor(sensors, isFan)
  const voltage =
    sensors.find(
      (sensor) =>
        isVoltage(sensor) && Number.isFinite(sensor.value) && sensor.value !== 0,
    ) ?? null

  const cards: readonly StatusCard[] = [
    {
      id: "cpu",
      label: "CPU LOAD",
      value: formatCpu(resources?.cpuLoad ?? null),
      highlighted: true,
      unsupported: false,
    },
    {
      id: "memory",
      label: "MEMORY",
      value: `${formatBytes(resources?.memUsedBytes ?? null)} / ${formatBytes(resources?.memTotalBytes ?? null)}`,
      highlighted: true,
      unsupported: false,
    },
    {
      id: "uptime",
      label: "UPTIME",
      value: resources?.uptime ?? "-",
      highlighted: false,
      unsupported: false,
    },
    {
      id: "board",
      label: "BOARD NAME",
      value: resources?.boardName ?? metadata?.boardName ?? "-",
      highlighted: false,
      unsupported: false,
    },
    {
      id: "routeros",
      label: "ROUTEROS VERSION",
      value: resources?.routerosVersion ?? metadata?.routerosVersion ?? "-",
      highlighted: false,
      unsupported: false,
    },
    {
      id: "architecture",
      label: "ARCHITECTURE",
      value: resources?.architectureName ?? metadata?.architectureName ?? "-",
      highlighted: false,
      unsupported: false,
    },
    {
      id: "temperature",
      label: "TEMPERATURE",
      value: sensorValue(temperature, "°C"),
      highlighted: true,
      unsupported: temperature === null,
    },
    {
      id: "fan",
      label: "FAN",
      value: sensorValue(fan, "RPM", true),
      highlighted: true,
      unsupported: fan === null,
    },
    {
      id: "voltage",
      label: "VOLTAGE",
      value: sensorValue(voltage, "V"),
      highlighted: true,
      unsupported: voltage === null,
    },
  ]

  return (
    <dl className={styles.grid} aria-label="MikroTik status">
      {cards.map((card) => (
        <div
          className={styles.card}
          data-testid={`mikrotik-status-${card.id}`}
          key={card.id}
        >
          <dt className={styles.label}>{card.label}</dt>
          <dd
            className={`${styles.value}${card.highlighted ? ` ${styles.highlighted}` : ""}${card.unsupported ? ` ${styles.unsupported}` : ""}`}
            title={card.value}
          >
            {card.value}
          </dd>
        </div>
      ))}
    </dl>
  )
}
