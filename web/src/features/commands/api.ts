import type { Action, Filed } from "../../api/types.gen";

export function commandId(_random?: (bytes: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>): string {
  throw new Error("not implemented");
}

export async function fileCommand(_node: number, _id: string, _action: Action): Promise<Filed> {
  throw new Error("not implemented");
}
