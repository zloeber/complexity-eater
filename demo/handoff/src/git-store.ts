import git from "isomorphic-git";
import http from "isomorphic-git/http/web";
import { MemoryFS } from "./memory-fs";

const AUTHOR = { name: "Complexity Eater", email: "handoff@complexityeater.invalid" };
const DIR = "/workspace";

export type ArtifactRepo = {
  info(): Promise<{ remote?: string; defaultBranch?: string; name?: string }>;
  createToken(
    scope?: "read" | "write",
    ttl?: number,
  ): Promise<{ plaintext: string; expiresAt?: string }>;
  readFile(args: { ref: string; path: string }): Promise<Blob | null>;
  log(opts?: { ref?: string; limit?: number; offset?: number }): Promise<unknown[]>;
  [Symbol.dispose](): void;
};

export type ArtifactsBinding = {
  create(
    name: string,
    opts?: { description?: string; readOnly?: boolean; setDefaultBranch?: string },
  ): Promise<{ name: string; remote: string; token: string; defaultBranch?: string }>;
  get(name: string): Promise<ArtifactRepo>;
  delete(name: string): Promise<unknown>;
  list(opts?: { limit?: number }): Promise<{ repos?: Array<{ name?: string; status?: string }> }>;
};

export type HistoryEntry = { hash: string; message: string };

function tokenSecret(token: string): string {
  return token.split("?expires=")[0] ?? token;
}

function auth(token: string) {
  const secret = tokenSecret(token);
  return () => ({ username: "x", password: secret });
}

async function writeTree(fs: MemoryFS, files: Record<string, string>) {
  for (const [path, content] of Object.entries(files)) {
    const text = content.endsWith("\n") ? content : `${content}\n`;
    await fs.promises.writeFile(`${DIR}/${path}`, text);
    await git.add({ fs: fs as never, dir: DIR, filepath: path });
  }
}

export async function pushNewRepo(
  remote: string,
  token: string,
  files: Record<string, string>,
  message: string,
): Promise<string> {
  const fs = new MemoryFS();
  await git.init({ fs: fs as never, dir: DIR, defaultBranch: "main" });
  await writeTree(fs, files);
  const oid = await git.commit({
    fs: fs as never,
    dir: DIR,
    message,
    author: AUTHOR,
  });
  const push = await git.push({
    fs: fs as never,
    http,
    dir: DIR,
    url: remote,
    ref: "main",
    remoteRef: "main",
    onAuth: auth(token),
  });
  if (push.ok === false) throw new Error("artifact push was rejected");
  return oid;
}

export async function pushUpdate(
  remote: string,
  token: string,
  files: Record<string, string>,
  message: string,
): Promise<string> {
  const fs = new MemoryFS();
  await git.clone({
    fs: fs as never,
    http,
    dir: DIR,
    url: remote,
    ref: "main",
    singleBranch: true,
    depth: 20,
    onAuth: auth(token),
  });
  await writeTree(fs, files);
  const oid = await git.commit({
    fs: fs as never,
    dir: DIR,
    message,
    author: AUTHOR,
  });
  const push = await git.push({
    fs: fs as never,
    http,
    dir: DIR,
    url: remote,
    ref: "main",
    remoteRef: "main",
    onAuth: auth(token),
  });
  if (push.ok === false) throw new Error("artifact push was rejected");
  return oid;
}

export async function readText(
  repo: ArtifactRepo,
  path: string,
  ref = "main",
): Promise<string | null> {
  const file = await repo.readFile({ ref, path });
  if (!file) return null;
  const text = await file.text();
  return text.replace(/\r\n/g, "\n");
}

export function historyFromLog(commits: unknown[]): HistoryEntry[] {
  return commits.map((commit) => {
    const record = commit && typeof commit === "object" ? (commit as Record<string, unknown>) : {};
    const hash = firstString(record.hash, record.oid, record.id, record.sha);
    const message = firstLine(firstString(record.message, record.summary, record.title));
    return { hash, message };
  });
}

function firstString(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return "";
}

function firstLine(value: string): string {
  return value.split("\n")[0]?.trim() ?? "";
}

export async function sleep(ms: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
}
