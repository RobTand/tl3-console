import type { Card, CaseFile } from "../../api/types.gen";

export function useCaseFile(_id: number | null, _card: Card | undefined): { file: CaseFile | undefined; error: string | null } {
  return { file: undefined, error: null };
}

export function useOpenRequest(): { id: number | null; open: (id: number) => void; close: () => void } {
  return { id: null, open: () => {}, close: () => {} };
}
