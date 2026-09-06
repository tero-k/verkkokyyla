import { useCallback, useState } from "react"
import { ConfirmDialog } from "../components/ConfirmDialog"

type PendingConfirm = {
  readonly message: string
  readonly resolve: (confirmed: boolean) => void
}

/**
 * Promise-based in-app confirmation. Render the returned `dialog` node once
 * near the root of the view; `confirm(message)` resolves true/false.
 */
export function useConfirmDialog() {
  const [pending, setPending] = useState<PendingConfirm | null>(null)

  const confirm = useCallback(
    (message: string) =>
      new Promise<boolean>((resolve) => {
        setPending({ message, resolve })
      }),
    [],
  )

  const settle = useCallback(
    (confirmed: boolean) => {
      pending?.resolve(confirmed)
      setPending(null)
    },
    [pending],
  )

  const dialog = pending ? (
    <ConfirmDialog
      message={pending.message}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  ) : null

  return { confirm, dialog }
}
