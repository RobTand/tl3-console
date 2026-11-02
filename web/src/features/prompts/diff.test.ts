import { lineChanges } from "./diff";

describe("lineChanges", () => {
  it.each([
    ["a\nb\nc", "a\nb\nc", 0, 0],
    ["a\nb\nc", "a\nB\nc", 1, 1],
    ["a\nc", "a\nb\nc", 1, 0],
    ["a\nb\nc", "c", 0, 2],
    ["", "new prompt", 1, 1],
    ["x\ny\nx\ny", "y\nx\ny\nx", 1, 1],
  ])("%j to %j adds %i and removes %i lines", (before, after, added, removed) => {
    expect(lineChanges(before, after)).toEqual({ added, removed });
  });
});
