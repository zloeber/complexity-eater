export async function sha256Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function randomId(prefix: string, bytes = 12): string {
  return `${prefix}${randomHex(bytes)}`;
}

export function randomToken(): string {
  return randomHex(32);
}

function randomHex(bytes: number): string {
  const buffer = new Uint8Array(bytes);
  crypto.getRandomValues(buffer);
  return [...buffer].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function scrub(error: unknown): string {
  const text = error instanceof Error ? error.message : "request failed";
  return text.replace(/art_v1_[A-Za-z0-9]+(\?expires=\d+)?/g, "[token]").slice(0, 300);
}

export function json(body: unknown, status = 200): Response {
  return Response.json(body, {
    status,
    headers: { "cache-control": "no-store" },
  });
}

export function bearer(request: Request): string | null {
  const header = request.headers.get("Authorization") ?? "";
  const match = /^Bearer ([0-9a-f]{64})$/.exec(header);
  return match?.[1] ?? null;
}

export async function readJson(request: Request): Promise<unknown> {
  const text = await request.text();
  if (text.length > 80_000) throw new PayloadTooLarge();
  if (!text) return {};
  return JSON.parse(text) as unknown;
}

export class PayloadTooLarge extends Error {
  constructor() {
    super("payload too large");
  }
}
