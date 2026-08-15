export function formatMetric(value: number | null, digits = 2): string {
  if (value === null || Number.isNaN(value)) return "-"
  return value.toFixed(digits)
}

export function formatTime(at: string): string {
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return "-"
  return date.toLocaleTimeString(undefined, { hour12: false })
}
