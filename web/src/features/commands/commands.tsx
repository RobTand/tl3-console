import type { ReactNode } from "react";
import type { CommandRecord } from "../../api/types.gen";
import type { fileCommand } from "./api";

export function CommandsProvider(props: { records: CommandRecord[]; file?: typeof fileCommand; children: ReactNode }) {
  return <>{props.children}</>;
}
