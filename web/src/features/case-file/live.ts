import { useQuery, useQueryClient } from "@tanstack/react-query";
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
    // A refetch keeps showing the last case file until the new one arrives.
    staleTime: Infinity,
    retry: (failures, error) => !(error instanceof NotFound) && failures < 2,
  });
  // A closed drawer gets no stream updates, so its case file is dropped and
  // reopening reads a fresh one.
  useEffect(() => {
    if (id === null) return;
    return () => client.removeQueries({ queryKey: key(id), exact: true });
  }, [client, id]);
  // The board replaces a card object only when the stream changed it. A
  // newly opened request is already being read.
  const shown = useRef({ id, card });
  useEffect(() => {
    const before = shown.current;
    shown.current = { id, card };
    if (id === null || before.id !== id || before.card === card) return;
    void client.invalidateQueries({ queryKey: key(id) });
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
    if (next === null) { url.searchParams.delete("request"); url.searchParams.delete("tab"); }
    else url.searchParams.set("request", String(next));
    window.history.pushState(null, "", url);
    setId(next);
  }, []);
  const open = useCallback((next: number) => go(next), [go]);
  const close = useCallback(() => go(null), [go]);
  return { id, open, close };
}
