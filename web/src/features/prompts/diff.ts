/**
 * The text as the diff compares it. A last line without a newline would show
 * as changed when a line is added after it, so both sides end with one.
 */
export function withFinalNewline(text: string): string {
  return text.endsWith("\n") ? text : `${text}\n`;
}

/** Lines added and removed between two texts, from their longest common subsequence of lines. */
export function lineChanges(before: string, after: string): { added: number; removed: number } {
  const a = withFinalNewline(before).split("\n");
  const b = withFinalNewline(after).split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const left = a.slice(start, endA);
  const right = b.slice(start, endB);
  let previous = new Uint32Array(right.length + 1);
  let current = new Uint32Array(right.length + 1);
  for (const line of left) {
    for (let j = 1; j <= right.length; j++) {
      current[j] = line === right[j - 1] ? previous[j - 1]! + 1 : Math.max(previous[j]!, current[j - 1]!);
    }
    [previous, current] = [current, previous];
  }
  const common = previous[right.length]!;
  return { added: right.length - common, removed: left.length - common };
}
