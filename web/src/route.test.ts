import { href, parseRoute } from "./route";

describe("routes", () => {
  it.each([
    ["", { page: "board" }],
    ["#/", { page: "board" }],
    ["#/prompts", { page: "prompts", role: null }],
    ["#/prompts/reviewer", { page: "prompts", role: "reviewer" }],
    ["#/unknown", { page: "board" }],
  ])("%j is %j", (hash, route) => {
    expect(parseRoute(hash)).toEqual(route);
  });

  it("round-trips through href", () => {
    for (const route of [{ page: "board" }, { page: "prompts", role: null }, { page: "prompts", role: "maker" }] as const) {
      expect(parseRoute(href(route))).toEqual(route);
    }
  });
});
