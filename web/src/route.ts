export type Route = { page: "board" } | { page: "prompts"; role: string | null };

export function parseRoute(_hash: string): Route {
  return { page: "board" };
}

export function href(_route: Route): string {
  return "#/";
}
