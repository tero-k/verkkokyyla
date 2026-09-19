import type { ComparedHopRow, LoadedTraceDto, TraceHopRow } from "../lib/types"
import { Button, Card, SectionHeader } from "./ui/ui"

import styles from "./TracerouteComparison.module.css"

type TracerouteComparisonProps = {
  readonly a: LoadedTraceDto
  readonly b: LoadedTraceDto
  readonly diff: readonly ComparedHopRow[]
  readonly onClear: () => void
}

function formatRtt(value: number | null): string {
  if (value === null) return "—"
  return `${value.toFixed(value < 10 ? 2 : 1)} ms`
}

function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleString(undefined, { hour12: false })
}

function formatStatus(status: ComparedHopRow["status"]): string {
  switch (status) {
    case "same":
      return "Same"
    case "changed":
      return "Changed"
    case "a-only":
      return "Only A"
    case "b-only":
      return "Only B"
  }
}

function statusBadgeClass(status: ComparedHopRow["status"]): string {
  switch (status) {
    case "same":
      return styles.sameBadge
    case "changed":
      return styles.changedBadge
    case "a-only":
    case "b-only":
      return styles.missingBadge
  }
}

function HopSnapshot({ hop }: { readonly hop: TraceHopRow | null }) {
  if (hop === null) {
    return <span className={styles.missingHop}>Not present in this trace</span>
  }

  return (
    <div className={styles.hopSnapshot}>
      <span className={styles.hopHost}>
        {hop.hostname ?? hop.address ?? "No response"}
      </span>
      <span className={styles.hopTelemetry}>
        {hop.address ?? "filtered"} · {formatRtt(hop.rtt1Ms)} · {formatRtt(hop.rtt2Ms)} ·{" "}
        {formatRtt(hop.rtt3Ms)}
      </span>
      {hop.annotation !== null && (
        <span className={styles.hopAnnotation}>{hop.annotation}</span>
      )}
    </div>
  )
}

function TraceIdentity({
  label,
  trace,
}: {
  readonly label: "A" | "B"
  readonly trace: LoadedTraceDto["trace"]
}) {
  return (
    <Card className={styles.identityCard}>
      <span className="vk-badge">Trace {label}</span>
      <div className={styles.identityCopy}>
        <span className={styles.identityTarget}>{trace.targetInput}</span>
        <span className={styles.identityMeta}>
          {trace.resolvedIp} · {formatDateTime(trace.startedAt)}
        </span>
      </div>
      <span className={styles.identityId}>#{trace.id}</span>
    </Card>
  )
}

export function TracerouteComparison({ a, b, diff, onClear }: TracerouteComparisonProps) {
  return (
    <div className={styles.wrapper} data-testid="trace-comparison-view">
      <div className={styles.header}>
        <SectionHeader title="Trace comparison" aside={`${diff.length} route positions`} />
        <Button small onClick={onClear} data-testid="trace-comparison-clear">
          Back
        </Button>
      </div>

      <div className={styles.identities}>
        <TraceIdentity label="A" trace={a.trace} />
        <TraceIdentity label="B" trace={b.trace} />
      </div>

      {diff.length === 0 ? (
        <Card className={styles.empty}>No hops to compare.</Card>
      ) : (
        <Card className={styles.tableWrapper}>
          <table className={styles.table} data-testid="trace-comparison-table">
            <thead>
              <tr>
                <th className={styles.hopCol}>Hop</th>
                <th className={styles.sideCol}>Trace A</th>
                <th className={styles.sideCol}>Trace B</th>
                <th className={styles.statusCol}>Status</th>
              </tr>
            </thead>
            <tbody>
              {diff.map((row) => (
                <tr
                  key={row.hop}
                  className={styles[row.status]}
                  data-testid="trace-comparison-row"
                >
                  <td className={styles.hopCol}>{row.hop}</td>
                  <td className={styles.sideCol}>
                    <HopSnapshot hop={row.a} />
                  </td>
                  <td className={styles.sideCol}>
                    <HopSnapshot hop={row.b} />
                  </td>
                  <td className={styles.statusCol}>
                    <span className={`vk-badge ${statusBadgeClass(row.status)}`}>
                      {formatStatus(row.status)}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  )
}
