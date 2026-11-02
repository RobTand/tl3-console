import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { NewPromptVersion, PromptRole, PromptVersion, RolePrompt } from "../../api/types.gen";

/** The console refuses writes without this header, which other sites can't send. */
const WRITE_HEADER = { "X-TL3-Console": "1" };

/** A refused request, with the console's reason. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function call<T>(path: string, init?: { method: "POST"; body?: unknown }): Promise<T> {
  const response = await fetch(path, {
    ...(init ? { method: init.method } : {}),
    headers: { ...WRITE_HEADER, ...(init?.body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(init?.body === undefined ? {} : { body: JSON.stringify(init.body) }),
  });
  if (!response.ok) {
    const reason = (await response.text()).trim();
    throw new ApiError(reason || `${response.status} ${response.statusText}`, response.status);
  }
  return (await response.json()) as T;
}

export const roleKey = (role: string) => ["prompts", role] as const;
const path = (role: string) => `/api/prompts/${encodeURIComponent(role)}`;

export function usePromptRoles() {
  return useQuery({ queryKey: ["prompts"], queryFn: () => call<PromptRole[]>("/api/prompts") });
}

export function useRolePrompt(role: string) {
  return useQuery({ queryKey: roleKey(role), queryFn: () => call<RolePrompt>(path(role)) });
}

/** One version's text, fetched when the page first needs it. Versions never change. */
export function usePromptVersion(role: string, id: number | null) {
  return useQuery({
    queryKey: [...roleKey(role), "version", id] as const,
    queryFn: () => call<PromptVersion>(`${path(role)}/versions/${id}`),
    enabled: id !== null,
    staleTime: Infinity,
  });
}

/** Saving, activating and keeping each return the role's new state. */
export function usePromptEdits(role: string) {
  const client = useQueryClient();
  const settle = {
    onSuccess: (prompt: RolePrompt) => {
      client.setQueryData(roleKey(role), prompt);
      void client.invalidateQueries({ queryKey: ["prompts"], exact: true });
    },
  };
  return {
    save: useMutation({
      mutationFn: (edit: NewPromptVersion) => call<RolePrompt>(`${path(role)}/versions`, { method: "POST", body: edit }),
      ...settle,
    }),
    activate: useMutation({
      mutationFn: ({ version, expected }: { version: number; expected: number }) =>
        call<RolePrompt>(`${path(role)}/activate/${version}`, { method: "POST", body: { expected } }),
      ...settle,
    }),
    keep: useMutation({
      mutationFn: (version: number) => call<RolePrompt>(`${path(role)}/keep/${version}`, { method: "POST" }),
      ...settle,
    }),
  };
}
