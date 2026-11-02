import { lineChanges } from "./diff";

describe("lineChanges", () => {
  it.each([
    ["a\nb\nc", "a\nb\nc", 0, 0],
    ["a\nb\nc", "a\nB\nc", 1, 1],
    ["a\nc", "a\nb\nc", 1, 0],
    ["a\nb\nc", "c", 0, 2],
    ["", "new prompt", 1, 1],
    ["x\ny\nx\ny", "y\nx\ny\nx", 1, 1],
    // A line typed after a final newline adds a blank line and the text, as the diff shows them.
    ["a\n", "a\n\nb", 2, 0],
    ["a", "a\n", 0, 0],
  ])("%j to %j adds %i and removes %i lines", (before, after, added, removed) => {
    expect(lineChanges(before, after)).toEqual({ added, removed });
  });
});
