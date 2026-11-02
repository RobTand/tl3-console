import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { NewPromptVersion, PromptRole, RolePrompt } from "../../api/types.gen";

/** The console refuses writes without this header, which other sites can't send. */
const WRITE_HEADER = { "X-TL3-Console": "1" };

async function call<T>(path: string, init?: { method: "POST"; body?: unknown }): Promise<T> {
  const response = await fetch(path, {
    ...(init ? { method: init.method } : {}),
    headers: { ...WRITE_HEADER, ...(init?.body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(init?.body === undefined ? {} : { body: JSON.stringify(init.body) }),
  });
  if (!response.ok) {
    throw new Error((await response.text()).trim() || `${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

const roleKey = (role: string) => ["prompts", role] as const;

export function usePromptRoles() {
  return useQuery({ queryKey: ["prompts"], queryFn: () => call<PromptRole[]>("/api/prompts") });
}

export function useRolePrompt(role: string | null) {
  return useQuery({
    queryKey: roleKey(role ?? ""),
    queryFn: () => call<RolePrompt>(`/api/prompts/${encodeURIComponent(role!)}`),
    enabled: role !== null,
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
  const path = `/api/prompts/${encodeURIComponent(role)}`;
  return {
    save: useMutation({
      mutationFn: (edit: NewPromptVersion) => call<RolePrompt>(`${path}/versions`, { method: "POST", body: edit }),
      ...settle,
    }),
    activate: useMutation({
      mutationFn: (version: number) => call<RolePrompt>(`${path}/activate/${version}`, { method: "POST" }),
      ...settle,
    }),
    keep: useMutation({
      mutationFn: (version: number) => call<RolePrompt>(`${path}/keep/${version}`, { method: "POST" }),
      ...settle,
    }),
  };
}
