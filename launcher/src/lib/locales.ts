// Languages the launcher can be set to. English only for now; the first-run step and the
// Settings row both read this list.

export interface Language {
  code: string;
  name: string;
}

export const LANGUAGES: Language[] = [{ code: "en", name: "English" }];

export const DEFAULT_LANGUAGE = "en";

export function languageName(code: string | null | undefined): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? LANGUAGES[0].name;
}
