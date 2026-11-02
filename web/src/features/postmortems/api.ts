import { useInfiniteQuery } from "@tanstack/react-query";
import type { Postmortem, PostmortemPage, RequestPostmortems, Recommendation, CodeGrade } from "../../api/types.gen";

const object = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const nullableString = (value: unknown) => value === null || typeof value === "string";
const nullableNumber = (value: unknown) => value === null || typeof value === "number";
function recommendation(value: unknown): value is Recommendation {
  return (
    object(value) &&
    typeof value.id === "number" &&
    typeof value.rank === "number" &&
    typeof value.seen_in === "number" &&
    ["fingerprint", "phase", "kind", "target", "operation", "before", "after", "evidence", "effect"].every(
      (key) => typeof value[key] === "string",
    )
  );
}
function postmortem(value: unknown): value is Postmortem {
  return (
    object(value) &&
    typeof value.id === "number" &&
    typeof value.issue === "number" &&
    typeof value.owed_ms === "number" &&
    ["node_id", "trigger", "repo", "title"].every((key) => typeof value[key] === "string") &&
    ["sample_share", "started_ms", "finished_ms", "failed_ms", "rubric_version"].every((key) =>
      nullableNumber(value[key]),
    ) &&
    ["failure", "run_key", "model", "summary", "notes"].every((key) => nullableString(value[key])) &&
    (value.cause === null || value.cause === "tl3" || value.cause === "outside") &&
    (value.status === "owed" || value.status === "running" || value.status === "done" || value.status === "failed") &&
    (value.reason === "needed_fix" ||
      value.reason === "retry" ||
      value.reason === "escalation" ||
      value.reason === "closed_without_merge" ||
      value.reason === "sampled" ||
      value.reason === "rob_asked") &&
    Array.isArray(value.recommendations) &&
    value.recommendations.every(recommendation)
  );
}
function grade(value: unknown): value is CodeGrade {
  return (
    object(value) &&
    typeof value.id === "number" &&
    typeof value.node_id === "string" &&
    typeof value.trigger === "string" &&
    typeof value.rubric_version === "number" &&
    typeof value.dimension === "string" &&
    nullableNumber(value.grade) &&
    typeof value.recorded_ms === "number" &&
    "fact" in value
  );
}
function page(value: unknown): value is PostmortemPage {
  return (
    object(value) &&
    Array.isArray(value.postmortems) &&
    value.postmortems.every(postmortem) &&
    nullableNumber(value.next_before)
  );
}
function request(value: unknown): value is RequestPostmortems {
  return (
    page(value) &&
    "grades" in value &&
    Array.isArray(value.grades) &&
    value.grades.every(grade) &&
    "next_grade_before" in value &&
    nullableNumber(value.next_grade_before) &&
    "active" in value &&
    typeof value.active === "boolean"
  );
}
async function read<T>(url: string, signal: AbortSignal, valid: (value: unknown) => value is T): Promise<T> {
  const response = await fetch(url, { signal, headers: { accept: "application/json" } });
  if (response.status === 404) throw new Error("This request isn't in the store.");
  if (!response.ok) throw new Error(`Postmortems couldn't be read (${response.status}).`);
  const value: unknown = await response.json();
  if (!valid(value)) throw new Error("The console returned an unreadable postmortem.");
  return value;
}
interface Cursor {
  before: number | null;
  gradeBefore: number | null;
}
const first: Cursor = { before: null, gradeBefore: null };
export function usePostmortems(id: number | null) {
  return useInfiniteQuery({
    queryKey: ["postmortems", "request", id],
    enabled: id !== null,
    initialPageParam: first,
    queryFn: ({ pageParam, signal }) => {
      const params = new URLSearchParams();
      if (pageParam.before !== null) params.set("before", String(pageParam.before));
      if (pageParam.gradeBefore !== null) params.set("grade_before", String(pageParam.gradeBefore));
      return read(`/api/requests/${id}/postmortems?${params}`, signal, request);
    },
    // IDs start at 1. Keep exhausted collections empty while the other pages.
    getNextPageParam: (last): Cursor | undefined =>
      last.next_before === null && last.next_grade_before === null
        ? undefined
        : { before: last.next_before ?? 1, gradeBefore: last.next_grade_before ?? 1 },
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    retry: false,
  });
}
export function usePostmortemList(filters: string) {
  const initial: number | null = null;
  return useInfiniteQuery({
    queryKey: ["postmortems", "list", filters],
    initialPageParam: initial,
    queryFn: ({ pageParam, signal }: { pageParam: number | null; signal: AbortSignal }) =>
      read(`/api/postmortems?${filters}${pageParam === null ? "" : `&before=${pageParam}`}`, signal, page),
    getNextPageParam: (last) => last.next_before ?? undefined,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    retry: false,
  });
}
