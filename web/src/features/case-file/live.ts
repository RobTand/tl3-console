import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Card, CaseFile } from "../../api/types.gen";

class NotFound extends Error {}

const key = (id: number | null) => ["case-file", id] as const;

/**
 * A request's case file. It is fetched when the drawer opens and again
 * whenever the event stream changes the request's card; there is no timer.
 */
export function useCaseFile(
  id: number | null,
  card: Card | undefined,
): { file: CaseFile | undefined; error: string | null } {
  const client = useQueryClient();
  const query = useQuery<CaseFile>({
    queryKey: key(id),
    enabled: id !== null,
    queryFn: async ({ signal }) => {
      const response = await fetch(`/api/requests/${id}`, { signal, headers: { accept: "application/json" } });
      if (response.status === 404) throw new NotFound();
      if (!response.ok) throw new Error(`The console answered ${response.status}`);
      return (await response.json()) as CaseFile;
    },
    // Keep showing the last case file while a newer one loads, so nothing jumps.
    placeholderData: keepPreviousData,
    staleTime: Infinity,
    retry: (failures, error) => !(error instanceof NotFound) && failures < 2,
  });
  // The board replaces a card object only when the stream changed it.
  const shown = useRef(card);
  useEffect(() => {
    if (shown.current === card) return;
    shown.current = card;
    if (id !== null) void client.invalidateQueries({ queryKey: key(id) });
  }, [card, client, id]);
  const error =
    query.error instanceof NotFound
      ? "This request isn't in the store."
      : query.error
        ? "The case file couldn't be read. It loads again with the next update."
        : null;
  return { file: query.data, error };
}

function readId(): number | null {
  const value = new URLSearchParams(window.location.search).get("request");
  return value !== null && /^\d+$/.test(value) ? Number(value) : null;
}

/** The open request lives in the address, so a link opens it and back closes it. */
export function useOpenRequest(): { id: number | null; open: (id: number) => void; close: () => void } {
  const [id, setId] = useState(readId);
  useEffect(() => {
    const follow = () => setId(readId());
    window.addEventListener("popstate", follow);
    return () => window.removeEventListener("popstate", follow);
  }, []);
  const go = useCallback((next: number | null) => {
    const url = new URL(window.location.href);
    if (next === null) url.searchParams.delete("request");
    else url.searchParams.set("request", String(next));
    window.history.pushState(null, "", url);
    setId(next);
  }, []);
  const open = useCallback((next: number) => go(next), [go]);
  const close = useCallback(() => go(null), [go]);
  return { id, open, close };
}
