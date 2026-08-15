// The app has exactly one tool. Route resolution is deliberately trivial:
// anything that is not the Ping hash still renders the Ping view.
export type Route = "ping" | "download-speed";

export interface NavItem {
  readonly route: Route;
  readonly label: string;
  readonly hash: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { route: "ping", label: "Ping", hash: "#/ping" },
  { route: "download-speed", label: "Download speed test", hash: "#/download-speed" },
];

export function resolveRoute(hash: string): Route {
  if (hash === "#/download-speed") return "download-speed";
  return "ping";
}
