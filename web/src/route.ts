import { useSyncExternalStore } from "react";

/** Pages live in the URL hash, so the console serves one HTML file for every page. */
export type Route = { page: "board" } | { page: "prompts"; role: string | null };

export function parseRoute(hash: string): Route {
  const [page, role] = hash.replace(/^#\/?/, "").split("/");
  if (page === "prompts") return { page: "prompts", role: role ? decodeURIComponent(role) : null };
  return { page: "board" };
}

export function href(route: Route): string {
  return route.page === "prompts" ? `#/prompts${route.role ? `/${encodeURIComponent(route.role)}` : ""}` : "#/";
}

function subscribe(changed: () => void) {
  window.addEventListener("hashchange", changed);
  return () => window.removeEventListener("hashchange", changed);
}

export function useRoute(): Route {
  return parseRoute(useSyncExternalStore(subscribe, () => window.location.hash));
}
