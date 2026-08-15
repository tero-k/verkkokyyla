// The app has exactly one tool. Route resolution is deliberately trivial:
// anything that is not the Ping hash still renders the Ping view.
export type Route = "ping";

export interface NavItem {
  readonly route: Route;
  readonly label: string;
  readonly hash: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { route: "ping", label: "Ping", hash: "#/ping" },
];

export function resolveRoute(_hash: string): Route {
  return "ping";
}
