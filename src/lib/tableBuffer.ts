import type { ProbeRow } from "./types"

export class TableBuffer {
  private readonly capacity: number
  private readonly rows: ProbeRow[] = []

  constructor(capacity: number) {
    this.capacity = capacity
  }

  add(row: ProbeRow): void {
    this.rows.push(row)
    while (this.rows.length > this.capacity) {
      this.rows.shift()
    }
  }

  all(): readonly ProbeRow[] {
    return this.rows
  }

  clear(): void {
    this.rows.length = 0
  }
}
