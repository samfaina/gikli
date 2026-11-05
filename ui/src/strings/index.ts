import { en, type Strings } from "./en";

export type StringKey = keyof Strings;

const strings: Strings = en;

/** The UI text for `key`, with each `{name}` replaced by `values[name]`. */
export function t(key: StringKey, values: Record<string, string> = {}): string {
  return strings[key].replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    name in values ? values[name]! : placeholder,
  );
}
