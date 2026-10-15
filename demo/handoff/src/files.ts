import type { ProfileId } from "./profiles";
import type { HandoffStatus } from "./access";

export const ORIGINAL_PATH = "original.md";

export function ensureTrailingNewline(value: string): string {
  return value.endsWith("\n") ? value : `${value}\n`;
}

export function normalizeDocument(value: string): string {
  return value.replace(/\r\n/g, "\n").trim();
}

export function sealUpdate(
  current: Record<string, string>,
  patch: Record<string, string>,
): Record<string, string> {
  if (ORIGINAL_PATH in patch) {
    throw new Error("original is sealed");
  }
  const next = { ...current, ...patch };
  if (
    current[ORIGINAL_PATH] !== undefined &&
    next[ORIGINAL_PATH] !== current[ORIGINAL_PATH]
  ) {
    throw new Error("original changed");
  }
  return next;
}

export function manifest(input: {
  handoffId: string;
  title: string;
  status: HandoffStatus;
  senderProfileId: ProfileId;
  recipientProfileId: ProfileId;
  updatedAt: string;
}): string {
  return `${JSON.stringify(input, null, 2)}\n`;
}

export function initialFiles(input: {
  handoffId: string;
  title: string;
  senderProfileId: ProfileId;
  recipientProfileId: ProfileId;
  markdown: string;
  updatedAt: string;
}): Record<string, string> {
  return {
    [ORIGINAL_PATH]: ensureTrailingNewline(input.markdown),
    "manifest.json": manifest({
      handoffId: input.handoffId,
      title: input.title,
      status: "sealed",
      senderProfileId: input.senderProfileId,
      recipientProfileId: input.recipientProfileId,
      updatedAt: input.updatedAt,
    }),
  };
}

export function appendQuestion(existing: string | undefined, question: string): string {
  const line = `- ${question.replace(/\s+/g, " ").trim()}`;
  if (!existing) return `# Questions\n\n${line}\n`;
  return `${ensureTrailingNewline(existing).replace(/\n$/, "")}\n${line}\n`;
}

export function parseQuestions(markdown: string | null | undefined): string[] {
  if (!markdown) return [];
  return markdown
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("- "))
    .map((line) => line.slice(2).trim())
    .filter(Boolean);
}

export function boundedText(
  value: unknown,
  min: number,
  max: number,
): string | null {
  if (typeof value !== "string") return null;
  const text = value.replace(/\0/g, "").replace(/\r\n/g, "\n");
  if (text.trim().length < min || text.length > max) return null;
  return text;
}

export function oneLine(value: unknown, max: number): string | null {
  const text = boundedText(value, 1, max);
  if (!text) return null;
  const line = text.replace(/\s+/g, " ").trim();
  if (!line || line.length > max) return null;
  return line;
}

export function titleFromMarkdown(markdown: string): string {
  const heading = /^#\s+(.+)$/m.exec(markdown);
  const title = (heading?.[1] ?? "Personal handoff").trim();
  return title.slice(0, 140);
}
