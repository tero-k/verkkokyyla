// Route resolution is deliberately trivial:
// anything unknown still renders the Ping view.
export type Route =
  | "ping"
  | "traceroute"
  | "mtu"
  | "download-speed"
  | "lan-scan"
  | "dns-tester"
  | "mikrotik"
  | "help";

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
  { route: "mtu", label: "MTU Discovery", hash: "#/mtu" },
  { route: "dns-tester", label: "DNS Toolkit", hash: "#/dns-tester" },
  { route: "mikrotik", label: "MikroTik", hash: "#/mikrotik" },
  { route: "help", label: "Help", hash: "#/help" },
];

// Sidebar grouping and Ctrl+digit shortcuts, mirroring the Claude Design
// sidebar mockup (MEASURE / DISCOVER / MIKROTIK groups with kbd hints).
export interface NavGroup {
  readonly label: string;
  readonly items: readonly { readonly route: Route; readonly shortcut: string }[];
}

export const NAV_GROUPS: readonly NavGroup[] = [
  {
    label: "Measure",
    items: [
      { route: "ping", shortcut: "Ctrl 1" },
      { route: "traceroute", shortcut: "Ctrl 2" },
      { route: "download-speed", shortcut: "Ctrl 3" },
    ],
  },
  {
    label: "Discover",
    items: [
      { route: "lan-scan", shortcut: "Ctrl 4" },
      { route: "mtu", shortcut: "Ctrl 5" },
      { route: "dns-tester", shortcut: "Ctrl 6" },
    ],
  },
  {
    label: "MikroTik",
    items: [{ route: "mikrotik", shortcut: "Ctrl 7" }],
  },
  {
    label: "Guide",
    items: [{ route: "help", shortcut: "Ctrl 8" }],
  },
];

export function resolveRoute(hash: string): Route {
  if (hash === "#/traceroute") return "traceroute";
  if (hash === "#/mtu") return "mtu";
  if (hash === "#/download-speed") return "download-speed";
  if (hash === "#/lan-scan") return "lan-scan";
  if (hash === "#/dns-tester") return "dns-tester";
  if (hash === "#/mikrotik") return "mikrotik";
  if (hash === "#/help") return "help";
  return "ping";
}
