import { ToggleGroup } from "radix-ui";
import { useMemo, useState, type ReactNode } from "react";
import type { PromptRole, PromptVersion, RolePrompt } from "../../api/types.gen";
import { href } from "../../route";
import { elapsed, useNow } from "../../time";
import { usePromptEdits, usePromptRoles, useRolePrompt } from "./api";
import { PromptDiff, PromptEditor } from "./CodeViews";
import { lineChanges } from "./diff";

const BUTTON =
  "inline-flex h-8 items-center rounded-md border border-border bg-surface px-2.5 text-sm text-text hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50";
const SMALL_BUTTON =
  "inline-flex h-7 items-center rounded-md border border-border bg-surface px-2 text-xs text-text hover:border-border-strong disabled:cursor-not-allowed disabled:opacity-50";
const PRIMARY =
  "inline-flex h-8 items-center rounded-md bg-accent px-3 text-sm font-medium text-surface hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50";

export function title(role: string): string {
  return role.charAt(0).toUpperCase() + role.slice(1);
}

function Chip(props: { tone: string; children: ReactNode }) {
  return (
    <span className={`inline-flex h-5 items-center rounded px-1.5 text-2xs font-medium whitespace-nowrap ${props.tone}`}>
      {props.children}
    </span>
  );
}

function Ago({ ms, now }: { ms: number; now: number }) {
  const at = new Date(ms);
  return (
    <time dateTime={at.toISOString()} title={at.toLocaleString()}>
      {elapsed(now - ms)} ago
    </time>
  );
}

/** Each role's system prompt: edit with a diff and a note, see the history, revert in one click. */
export function PromptsPage({ role }: { role: string | null }) {
  const roles = usePromptRoles();
  const shown = role ?? roles.data?.[0]?.role ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 lg:flex-row">
      <RoleList roles={roles.data ?? []} selected={shown} />
      {roles.error ? (
        <Notice tone="text-danger">{roles.error.message}</Notice>
      ) : shown === null ? (
        <Notice tone="text-muted">{roles.isPending ? "Loading prompts…" : "No role has a prompt yet."}</Notice>
      ) : (
        <RolePanel key={shown} role={shown} />
      )}
    </div>
  );
}

function Notice({ tone, children }: { tone: string; children: ReactNode }) {
  return <p className={`flex flex-1 items-center justify-center text-sm ${tone}`}>{children}</p>;
}

function RoleList({ roles, selected }: { roles: PromptRole[]; selected: string | null }) {
  return (
    <nav aria-label="Roles" className="flex shrink-0 gap-1 overflow-x-auto lg:w-44 lg:flex-col">
      {roles.map((role) => (
        <a
          key={role.role}
          href={href({ page: "prompts", role: role.role })}
          aria-current={role.role === selected ? "page" : undefined}
          className="flex min-w-36 flex-col gap-0.5 rounded-md border border-transparent px-2.5 py-1.5 hover:border-border aria-[current=page]:border-border aria-[current=page]:bg-surface"
        >
          <span className="flex items-center gap-1.5 text-sm font-medium text-text">
            {title(role.role)}
            {role.builtin_update && <Chip tone="bg-info-soft text-info">Update</Chip>}
          </span>
          <span className="text-xs text-muted">
            v{role.active_number} · {role.active_builtin ? "Built-in" : "Edited"}
          </span>
        </a>
      ))}
    </nav>
  );
}

function RolePanel({ role }: { role: string }) {
  const prompt = useRolePrompt(role);
  if (prompt.error) return <Notice tone="text-danger">{prompt.error.message}</Notice>;
  if (!prompt.data) return <Notice tone="text-muted">Loading the {role} prompt…</Notice>;
  return <Workspace prompt={prompt.data} />;
}

function Workspace({ prompt }: { prompt: RolePrompt }) {
  const now = useNow();
  const edits = usePromptEdits(prompt.role);
  const versions = new Map(prompt.versions.map((version) => [version.id, version]));
  const active = versions.get(prompt.active)!;
  const update = prompt.builtin_update === null ? null : versions.get(prompt.builtin_update) ?? null;
  const [draft, setDraft] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [view, setView] = useState<"edit" | "changes">("edit");
  const [compare, setCompare] = useState<PromptVersion | null>(null);
  // Discarding remounts the editor on the active text.
  const [generation, setGeneration] = useState(0);
  const text = draft ?? active.text;
  const changed = text !== active.text;
  const counts = useMemo(() => lineChanges(active.text, text), [active.text, text]);
  const next = (prompt.versions[0]?.number ?? 0) + 1;
  const pending = edits.save.isPending || edits.activate.isPending || edits.keep.isPending;
  const failure = edits.save.error ?? edits.activate.error ?? edits.keep.error;
  const blocked = changed ? "Save or discard your edit first" : undefined;
  const label = `${title(prompt.role)} prompt`;

  const reset = () => {
    setDraft(null);
    setNote("");
    setView("edit");
    setGeneration((value) => value + 1);
  };
  const activate = (version: PromptVersion) => {
    edits.activate.mutate(version.id, { onSuccess: () => setCompare(null) });
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-3 lg:flex-row">
      <section aria-label={label} className="flex min-h-[32rem] min-w-0 flex-1 flex-col gap-2.5">
        <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 className="text-base font-semibold">{label}</h2>
          <p className="text-xs text-muted">
            v{active.number} active · {active.builtin ? "built-in" : "edited"} · set by {prompt.activated_by}{" "}
            <Ago ms={prompt.activated_ms} now={now} />
          </p>
        </header>
        {update && (
          <section
            aria-label="Built-in update"
            className="flex flex-wrap items-center gap-2 rounded-md border border-info/30 bg-info-soft px-3 py-2 text-xs text-info"
          >
            <p className="mr-auto">
              A deploy shipped built-in v{update.number} while v{active.number} is active. New runs keep using v
              {active.number} until you choose.
            </p>
            <button type="button" className={SMALL_BUTTON} onClick={() => setCompare(update)}>
              Compare
            </button>
            <button
              type="button"
              className={SMALL_BUTTON}
              disabled={pending || changed}
              title={blocked}
              onClick={() => activate(update)}
            >
              Use built-in v{update.number}
            </button>
            <button
              type="button"
              className={SMALL_BUTTON}
              disabled={pending}
              onClick={() => edits.keep.mutate(update.id, { onSuccess: () => setCompare(null) })}
            >
              Keep v{active.number}
            </button>
          </section>
        )}
        {compare ? (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="mr-auto text-sm font-medium">
                v{compare.number} compared with active v{active.number}
              </h3>
              {compare.id !== active.id && (
                <button
                  type="button"
                  className={BUTTON}
                  disabled={pending || changed}
                  title={blocked}
                  onClick={() => activate(compare)}
                >
                  {compare.number < active.number ? "Revert to" : "Activate"} v{compare.number}
                </button>
              )}
              <button type="button" className={BUTTON} onClick={() => setCompare(null)}>
                Close
              </button>
            </div>
            <div className="min-h-0 flex-1">
              <PromptDiff
                before={active.text}
                after={compare.text}
                label={`Changes from v${active.number} to v${compare.number}`}
              />
            </div>
          </>
        ) : (
          <>
            <ToggleGroup.Root
              type="single"
              aria-label="View"
              value={view}
              onValueChange={(value) => value && setView(value as "edit" | "changes")}
              className="inline-flex h-8 self-start rounded-md border border-border bg-surface p-0.5"
            >
              <ToggleGroup.Item
                value="edit"
                className="rounded px-2.5 text-xs text-muted data-[state=on]:bg-surface-2 data-[state=on]:text-text"
              >
                Edit
              </ToggleGroup.Item>
              <ToggleGroup.Item
                value="changes"
                className="rounded px-2.5 text-xs text-muted data-[state=on]:bg-surface-2 data-[state=on]:text-text"
              >
                Changes{changed && ` +${counts.added} −${counts.removed}`}
              </ToggleGroup.Item>
            </ToggleGroup.Root>
            <div className="min-h-0 flex-1">
              {/* The editor stays mounted while the diff shows, so undo history survives. */}
              <div className={view === "edit" ? "h-full" : "hidden"}>
                <PromptEditor
                  key={`${prompt.active}:${generation}`}
                  initial={active.text}
                  label={`${label}, Markdown`}
                  onChange={setDraft}
                />
              </div>
              {view === "changes" &&
                (changed ? (
                  <PromptDiff before={active.text} after={text} label={`Changes from v${active.number}`} />
                ) : (
                  <Notice tone="text-muted">No changes against v{active.number}.</Notice>
                ))}
            </div>
            <form
              aria-label="Save a new version"
              className="flex flex-wrap items-center gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                edits.save.mutate({ text, note, base: active.id }, { onSuccess: reset });
              }}
            >
              <input
                aria-label="Note"
                required
                placeholder="Why this change? A note is required."
                value={note}
                onChange={(event) => setNote(event.target.value)}
                className="h-8 min-w-48 flex-1 rounded-md border border-border bg-surface px-2.5 text-sm text-text placeholder:text-faint hover:border-border-strong"
              />
              <button type="button" className={BUTTON} disabled={!changed || pending} onClick={reset}>
                Discard
              </button>
              <button type="submit" className={PRIMARY} disabled={!changed || note.trim() === "" || pending}>
                Save as v{next}
              </button>
            </form>
          </>
        )}
        {failure && (
          <p role="alert" className="rounded-md bg-danger-soft px-3 py-2 text-xs text-danger">
            {failure.message}
          </p>
        )}
      </section>
      <aside aria-label="Version history" className="flex min-h-0 shrink-0 flex-col gap-2 lg:w-80">
        <h3 className="text-sm font-medium">History</h3>
        <ol className="flex min-h-0 flex-col gap-1.5 overflow-y-auto">
          {prompt.versions.map((version) => (
            <li
              key={version.id}
              aria-label={`Version ${version.number}`}
              className="rounded-md border border-border bg-surface p-2.5 shadow-[var(--shadow)]"
            >
              <div className="flex items-center gap-1.5">
                <span className="font-mono text-sm font-medium">v{version.number}</span>
                {version.id === active.id && <Chip tone="bg-accent-soft text-accent">Active</Chip>}
                {version.builtin && <Chip tone="bg-surface-2 text-muted">Built-in</Chip>}
                <span className="ml-auto text-2xs text-faint">
                  <Ago ms={version.created_ms} now={now} />
                </span>
              </div>
              <p className="mt-1 text-xs text-muted">{version.author}</p>
              <p className="mt-0.5 text-xs text-text">{version.note}</p>
              {version.id !== active.id && (
                <div className="mt-2 flex gap-1.5">
                  <button type="button" className={SMALL_BUTTON} onClick={() => setCompare(version)}>
                    Compare
                  </button>
                  <button
                    type="button"
                    className={SMALL_BUTTON}
                    disabled={pending || changed}
                    title={blocked}
                    onClick={() => activate(version)}
                  >
                    {version.number < active.number ? "Revert to" : "Activate"} v{version.number}
                  </button>
                </div>
              )}
            </li>
          ))}
        </ol>
      </aside>
    </div>
  );
}
