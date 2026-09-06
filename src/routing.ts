// The app has six tools. Route resolution is deliberately trivial:
// anything unknown still renders the Ping view.
export type Route = "ping" | "traceroute" | "download-speed" | "lan-scan" | "dns-tester" | "mikrotik";

export interface NavItem {
  readonly route: Route;
  readonly label: string;
  readonly hash: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { route: "ping", label: "Ping", hash: "#/ping" },
  { route: "traceroute", label: "Traceroute", hash: "#/traceroute" },
  { route: "download-speed", label: "Web Benchmark", hash: "#/download-speed" },
  { route: "lan-scan", label: "Network scanner", hash: "#/lan-scan" },
  { route: "dns-tester", label: "DNS Toolkit", hash: "#/dns-tester" },
  { route: "mikrotik", label: "MikroTik", hash: "#/mikrotik" },
];

export function resolveRoute(hash: string): Route {
  if (hash === "#/traceroute") return "traceroute";
  if (hash === "#/download-speed") return "download-speed";
  if (hash === "#/lan-scan") return "lan-scan";
  if (hash === "#/dns-tester") return "dns-tester";
  if (hash === "#/mikrotik") return "mikrotik";
  return "ping";
}
