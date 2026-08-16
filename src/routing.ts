// The app has three tools. Route resolution is deliberately trivial:
// anything unknown still renders the Ping view.
export type Route = "ping" | "traceroute" | "download-speed";

export interface NavItem {
  readonly route: Route;
  readonly label: string;
  readonly hash: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { route: "ping", label: "Ping", hash: "#/ping" },
  { route: "traceroute", label: "Traceroute", hash: "#/traceroute" },
  { route: "download-speed", label: "Web page speed test", hash: "#/download-speed" },
];

export function resolveRoute(hash: string): Route {
  if (hash === "#/traceroute") return "traceroute";
  if (hash === "#/download-speed") return "download-speed";
  return "ping";
}
