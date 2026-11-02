import type { Action, CommandRequest, Filed } from "../../api/types.gen";

/** The console refuses writes without this header, which a cross-site form can't send. */
export const CONSOLE_HEADER = "X-TL3-Console";

/**
 * A version 4 UUID. `crypto.randomUUID` exists only in secure contexts, and
 * the console is served over plain HTTP on the LAN.
 */
export function commandId(
  random: (bytes: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer> = (bytes) => crypto.getRandomValues(bytes),
): string {
  const bytes = random(new Uint8Array(16));
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** File a command for a request. Resolves once the console has filed it for tl3d. */
export async function fileCommand(node: number, id: string, action: Action): Promise<Filed> {
  const body: CommandRequest = { id, action };
  const response = await fetch(`/api/requests/${node}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json", [CONSOLE_HEADER]: "1" },
    body: JSON.stringify(body),
  });
  if (response.status !== 202) {
    const reason = (await response.text()).trim();
    throw new Error(reason || `The console answered ${response.status}.`);
  }
  return (await response.json()) as Filed;
}
