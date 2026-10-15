import { DurableObject } from "cloudflare:workers";
import {
  decideAccess,
  decideRedeem,
  decideTransition,
  hasCredentialQuery,
  publicInviteError,
  routeApi,
  type Action,
  type HandoffStatus,
  type Role,
} from "./access";
import {
  appendQuestion,
  boundedText,
  initialFiles,
  manifest,
  normalizeDocument,
  oneLine,
  parseQuestions,
  sealUpdate,
  titleFromMarkdown,
} from "./files";
import {
  historyFromLog,
  pushNewRepo,
  pushUpdate,
  readText,
  sleep,
  type ArtifactsBinding,
  type HistoryEntry,
} from "./git-store";
import { bearer, json, PayloadTooLarge, randomId, randomToken, readJson, scrub, sha256Hex } from "./http";
import { interpret, type Interpretation } from "./interpret";
import { isProfileId, profile, PROFILES, type ProfileId } from "./profiles";
import { SAMPLE_MARKDOWN, SAMPLE_TITLE } from "./sample";

const NAMESPACE = "complexity-eater";
const INVITE_MS = 60 * 60 * 1000;
const SESSION_MS = 8 * 60 * 60 * 1000;
const FILE_PATHS = [
  "original.md",
  "manifest.json",
  "interpretation.json",
  "questions.md",
  "response.md",
  "return.json",
  "acknowledgement.json",
];

type Row = Record<string, string | number | null>;

export interface Env {
  COORDINATOR: DurableObjectNamespace<HandoffCoordinator>;
  ARTIFACTS?: ArtifactsBinding;
  ASSETS: { fetch(request: Request): Promise<Response> };
  DEMO_RESET_TOKEN?: string;
}

type HandoffRow = {
  id: string;
  repo_name: string;
  title: string;
  status: HandoffStatus;
  sender_profile: ProfileId;
  recipient_profile: ProfileId;
  original_sha: string;
  original_commit: string | null;
  head_commit: string | null;
  created_at: string;
  updated_at: string;
};

type SessionRow = {
  token_hash: string;
  handoff_id: string;
  role: Role;
  expires_at: number;
};

export class HandoffCoordinator extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.ctx.storage.sql.exec(`
      CREATE TABLE IF NOT EXISTS handoffs (
        id TEXT PRIMARY KEY,
        repo_name TEXT NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        sender_profile TEXT NOT NULL,
        recipient_profile TEXT NOT NULL,
        original_sha TEXT NOT NULL,
        original_commit TEXT,
        head_commit TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS sessions (
        token_hash TEXT PRIMARY KEY,
        handoff_id TEXT NOT NULL,
        role TEXT NOT NULL,
        expires_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS invitations (
        code_hash TEXT PRIMARY KEY,
        handoff_id TEXT NOT NULL,
        expires_at INTEGER NOT NULL,
        spent INTEGER NOT NULL DEFAULT 0,
        revoked INTEGER NOT NULL DEFAULT 0
      );
      CREATE TABLE IF NOT EXISTS events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        handoff_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        message TEXT NOT NULL,
        commit_hash TEXT,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS documents (
        repo_name TEXT NOT NULL,
        path TEXT NOT NULL,
        content TEXT NOT NULL,
        PRIMARY KEY (repo_name, path)
      );
      CREATE TABLE IF NOT EXISTS revisions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        repo_name TEXT NOT NULL,
        hash TEXT NOT NULL,
        message TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
    `);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (hasCredentialQuery(url)) return json({ error: "credentials_in_query" }, 400);
    const route = routeApi(request.method, url.pathname);
    if (!route) return json({ error: "not_found" }, 404);
    try {
      if (route.kind === "health") return await this.health();
      if (route.kind === "profiles") return json({ profiles: Object.values(PROFILES), simulatedAgent: true });
      if (route.kind === "sample") return json({ title: SAMPLE_TITLE, markdown: SAMPLE_MARKDOWN });
      if (route.kind === "create") return await this.create(request);
      if (route.kind === "redeem") return await this.redeem(request);
      if (route.kind === "reset") return await this.reset(request);
      if (route.action === "history") return await this.history(request, route.id);
      return await this.mutate(request, route.id, route.action);
    } catch (error) {
      if (error instanceof PayloadTooLarge) return json({ error: "limit_exceeded" }, 413);
      if (error instanceof SyntaxError) return json({ error: "invalid_json" }, 400);
      console.error(scrub(error));
      return json({ error: "persistence_failed" }, 500);
    }
  }

  private async health(): Promise<Response> {
    if (!this.env.ARTIFACTS) {
      return json({
        ok: true,
        coordination: "cloudflare-workers",
        persistence: "cloudflare-durable-object",
        artifacts: "unavailable",
        namespace: NAMESPACE,
      });
    }
    try {
      const page = await this.env.ARTIFACTS.list({ limit: 1 });
      return json({
        ok: true,
        coordination: "cloudflare-workers",
        persistence: "cloudflare-artifacts",
        namespace: NAMESPACE,
        artifacts: "binding",
        repoSample: page.repos?.length ?? 0,
      });
    } catch (error) {
      console.error(scrub(error));
      return json({ ok: false, error: "artifacts_unavailable" }, 503);
    }
  }

  private async create(request: Request): Promise<Response> {
    const body = (await readJson(request)) as Record<string, unknown>;
    if (!isProfileId(body.senderProfileId) || !isProfileId(body.recipientProfileId)) {
      return json({ error: "invalid_profile" }, 400);
    }
    if (body.senderProfileId === body.recipientProfileId) {
      return json({ error: "profiles_must_differ" }, 400);
    }
    const markdown = boundedText(body.markdown, 1, 48_000);
    if (!markdown) return json({ error: "invalid_markdown" }, 400);
    const sender = body.senderProfileId;
    const recipient = body.recipientProfileId;
    const now = new Date().toISOString();
    const id = randomId("h");
    const repoName = `ceh${id.slice(1)}`;
    const title = titleFromMarkdown(markdown);
    const files = initialFiles({
      handoffId: id,
      title,
      senderProfileId: sender,
      recipientProfileId: recipient,
      markdown,
      updatedAt: now,
    });
    let createdRemote: { remote: string; token: string } | null = null;
    if (this.env.ARTIFACTS) {
      const created = await this.env.ARTIFACTS.create(repoName, {
        description: `Complexity Eater handoff ${id}`,
        setDefaultBranch: "main",
      });
      createdRemote = { remote: created.remote, token: created.token };
    }
    try {
      const oid = createdRemote
        ? await pushNewRepo(createdRemote.remote, createdRemote.token, files, "Seal original handoff")
        : await this.localSave(repoName, files, "Seal original handoff", true);
      const stored = await this.readRequired(repoName, "original.md");
      if (!stored || normalizeDocument(stored) !== normalizeDocument(markdown)) {
        throw new Error("original readback mismatch");
      }
      const token = randomToken();
      const originalSha = await sha256Hex(normalizeDocument(markdown));
      this.sql(
        `INSERT INTO handoffs (id, repo_name, title, status, sender_profile, recipient_profile, original_sha, original_commit, head_commit, created_at, updated_at)
         VALUES (?, ?, ?, 'sealed', ?, ?, ?, ?, ?, ?, ?)`,
        id,
        repoName,
        title,
        sender,
        recipient,
        originalSha,
        oid,
        oid,
        now,
        now,
      );
      await this.saveSession(token, id, "sender");
      this.addEvent(id, "sealed", "Sender sealed the original markdown.", oid, now);
      const row = this.handoff(id);
      if (!row) return json({ error: "persistence_failed" }, 500);
      return json({ sessionToken: token, handoff: await this.view(row, "sender") }, 201);
    } catch (error) {
      await this.removeRepo(repoName);
      this.sql("DELETE FROM sessions WHERE handoff_id = ?", id);
      this.sql("DELETE FROM invitations WHERE handoff_id = ?", id);
      this.sql("DELETE FROM events WHERE handoff_id = ?", id);
      this.sql("DELETE FROM handoffs WHERE id = ?", id);
      throw error;
    }
  }

  private async redeem(request: Request): Promise<Response> {
    const body = (await readJson(request)) as Record<string, unknown>;
    const handoffId = typeof body.handoffId === "string" ? body.handoffId : "";
    const code = typeof body.code === "string" ? body.code : "";
    if (!/^h[0-9a-f]{24}$/.test(handoffId) || !/^[0-9a-f]{64}$/.test(code)) {
      return json({ error: publicInviteError("missing") }, 401);
    }
    const codeHash = await sha256Hex(code);
    const invite = this.one(
      "SELECT code_hash, handoff_id, expires_at, spent, revoked FROM invitations WHERE code_hash = ?",
      codeHash,
    );
    const decision = decideRedeem({
      invite: invite
        ? {
            handoffId: String(invite.handoff_id),
            expiresAt: Number(invite.expires_at),
            spent: Number(invite.spent) === 1,
            revoked: Number(invite.revoked) === 1,
          }
        : null,
      requestedHandoffId: handoffId,
      now: Date.now(),
    });
    if (decision !== "ok") return json({ error: publicInviteError(decision) }, 401);
    const row = this.handoff(handoffId);
    if (!row) return json({ error: publicInviteError("missing") }, 401);
    let head = row.head_commit;
    if (row.status === "sealed" || row.status === "invited") {
      const current = await this.currentFiles(row.repo_name);
      const reading = interpret(normalizeDocument(current["original.md"] ?? ""), row.recipient_profile, {
        source: "original",
        counterpart: profile(row.sender_profile).name,
      });
      const now = new Date().toISOString();
      const patch = sealUpdate(current, {
        "interpretation.json": `${JSON.stringify(reading, null, 2)}\n`,
        "manifest.json": manifest({
          handoffId: row.id,
          title: row.title,
          status: "interpreted",
          senderProfileId: row.sender_profile,
          recipientProfileId: row.recipient_profile,
          updatedAt: now,
        }),
      });
      head = await this.commit(row.repo_name, {
        "interpretation.json": patch["interpretation.json"] ?? "",
        "manifest.json": patch["manifest.json"] ?? "",
      }, "Record recipient interpretation");
      this.sql(
        "UPDATE handoffs SET status = 'interpreted', head_commit = ?, updated_at = ? WHERE id = ?",
        head,
        now,
        row.id,
      );
      this.addEvent(row.id, "interpreted", "Recipient agent recorded a simulated interpretation.", head, now);
    }
    this.sql("UPDATE invitations SET spent = 1 WHERE code_hash = ?", codeHash);
    const token = randomToken();
    await this.saveSession(token, row.id, "recipient");
    const fresh = this.handoff(row.id);
    if (!fresh) return json({ error: "persistence_failed" }, 500);
    return json({ sessionToken: token, handoff: await this.view(fresh, "recipient") });
  }

  private async mutate(request: Request, id: string, action: Action): Promise<Response> {
    const auth = await this.authorize(request, id, action);
    if ("error" in auth && auth.error) {
      const status = auth.error === "unauthorized" ? 401 : auth.error === "forbidden" ? 403 : auth.error === "conflict" ? 409 : 404;
      return json({ error: auth.error === "conflict" ? "conflict" : auth.error }, status);
    }
    const row = auth.row;
    if (!row) return json({ error: "not_found" }, 404);
    if (action === "read") return json({ handoff: await this.view(row, auth.role) });
    if (action === "preview") return await this.preview(request, row);
    if (action === "invite") return await this.invite(row);
    if (action === "revoke") return await this.revoke(row);
    if (action === "question") return await this.question(request, row);
    if (action === "respond") return await this.respond(request, row);
    if (action === "acknowledge") return await this.acknowledge(row);
    return json({ error: "not_found" }, 404);
  }

  private async invite(row: HandoffRow): Promise<Response> {
    const nowMs = Date.now();
    const now = new Date(nowMs).toISOString();
    this.sql(
      "UPDATE invitations SET revoked = 1 WHERE handoff_id = ? AND spent = 0 AND revoked = 0",
      row.id,
    );
    const code = randomToken();
    const codeHash = await sha256Hex(code);
    const expiresAt = nowMs + INVITE_MS;
    this.sql(
      "INSERT INTO invitations (code_hash, handoff_id, expires_at, spent, revoked) VALUES (?, ?, ?, 0, 0)",
      codeHash,
      row.id,
      expiresAt,
    );
    if (row.status === "sealed") {
      this.sql("UPDATE handoffs SET status = 'invited', updated_at = ? WHERE id = ?", now, row.id);
    }
    this.addEvent(row.id, "invited", "Sender created a one-use invitation.", null, now);
    const fresh = this.handoff(row.id) ?? row;
    return json({
      inviteCode: code,
      expiresAt: new Date(expiresAt).toISOString(),
      handoffPath: `/h/${row.id}`,
      handoff: await this.view(fresh, "sender"),
    });
  }

  private async revoke(row: HandoffRow): Promise<Response> {
    const now = new Date().toISOString();
    this.sql(
      "UPDATE invitations SET revoked = 1 WHERE handoff_id = ? AND spent = 0 AND revoked = 0",
      row.id,
    );
    this.addEvent(row.id, "revoked", "Sender revoked the open invitation.", null, now);
    return json({ handoff: await this.view(row, "sender") });
  }

  private async question(request: Request, row: HandoffRow): Promise<Response> {
    const body = (await readJson(request)) as Record<string, unknown>;
    const question = oneLine(body.text, 500);
    if (!question) return json({ error: "invalid_question" }, 400);
    const existingQuestions = parseQuestions(await this.readPath(row.repo_name, "questions.md"));
    if (existingQuestions.length >= 30) return json({ error: "limit_exceeded" }, 413);
    const current = await this.currentFiles(row.repo_name);
    const nextQuestions = appendQuestion(current["questions.md"], question);
    const now = new Date().toISOString();
    const patch = sealUpdate(current, {
      "questions.md": nextQuestions,
      "manifest.json": manifest({
        handoffId: row.id,
        title: row.title,
        status: row.status,
        senderProfileId: row.sender_profile,
        recipientProfileId: row.recipient_profile,
        updatedAt: now,
      }),
    });
    const head = await this.commit(row.repo_name, {
      "questions.md": patch["questions.md"] ?? "",
      "manifest.json": patch["manifest.json"] ?? "",
    }, "Add recipient question");
    await this.assertOriginal(row);
    this.sql("UPDATE handoffs SET head_commit = ?, updated_at = ? WHERE id = ?", head, now, row.id);
    this.addEvent(row.id, "question", "Recipient added a question.", head, now);
    const fresh = this.handoff(row.id) ?? row;
    return json({ handoff: await this.view(fresh, "recipient") });
  }

  private async respond(request: Request, row: HandoffRow): Promise<Response> {
    const body = (await readJson(request)) as Record<string, unknown>;
    const response = boundedText(body.markdown, 1, 16_000);
    if (!response) return json({ error: "invalid_markdown" }, 400);
    const reading = interpret(response, row.sender_profile, {
      source: "response",
      counterpart: profile(row.recipient_profile).name,
    });
    const current = await this.currentFiles(row.repo_name);
    const now = new Date().toISOString();
    const patch = sealUpdate(current, {
      "response.md": response.endsWith("\n") ? response : `${response}\n`,
      "return.json": `${JSON.stringify(reading, null, 2)}\n`,
      "manifest.json": manifest({
        handoffId: row.id,
        title: row.title,
        status: "returned",
        senderProfileId: row.sender_profile,
        recipientProfileId: row.recipient_profile,
        updatedAt: now,
      }),
    });
    const head = await this.commit(row.repo_name, {
      "response.md": patch["response.md"] ?? "",
      "return.json": patch["return.json"] ?? "",
      "manifest.json": patch["manifest.json"] ?? "",
    }, "Record return handoff");
    await this.assertOriginal(row);
    this.sql(
      "UPDATE handoffs SET status = 'returned', head_commit = ?, updated_at = ? WHERE id = ?",
      head,
      now,
      row.id,
    );
    this.addEvent(row.id, "returned", "Recipient sent a return handoff.", head, now);
    const fresh = this.handoff(row.id) ?? row;
    return json({ handoff: await this.view(fresh, "recipient") });
  }

  private async acknowledge(row: HandoffRow): Promise<Response> {
    const now = new Date().toISOString();
    const current = await this.currentFiles(row.repo_name);
    const note = {
      byProfileId: row.sender_profile,
      at: now,
      note: "Sender acknowledged the return handoff.",
    };
    const patch = sealUpdate(current, {
      "acknowledgement.json": `${JSON.stringify(note, null, 2)}\n`,
      "manifest.json": manifest({
        handoffId: row.id,
        title: row.title,
        status: "acknowledged",
        senderProfileId: row.sender_profile,
        recipientProfileId: row.recipient_profile,
        updatedAt: now,
      }),
    });
    const head = await this.commit(row.repo_name, {
      "acknowledgement.json": patch["acknowledgement.json"] ?? "",
      "manifest.json": patch["manifest.json"] ?? "",
    }, "Record sender acknowledgement");
    await this.assertOriginal(row);
    this.sql(
      "UPDATE handoffs SET status = 'acknowledged', head_commit = ?, updated_at = ? WHERE id = ?",
      head,
      now,
      row.id,
    );
    this.addEvent(row.id, "acknowledged", "Sender acknowledged the return.", head, now);
    const fresh = this.handoff(row.id) ?? row;
    return json({ handoff: await this.view(fresh, "sender") });
  }

  private async preview(request: Request, row: HandoffRow): Promise<Response> {
    const body = (await readJson(request)) as Record<string, unknown>;
    if (!isProfileId(body.profileId)) return json({ error: "invalid_profile" }, 400);
    const source = body.source === "response" ? "response" : "original";
    const path = source === "response" ? "response.md" : "original.md";
    const text = await this.readPath(row.repo_name, path);
    if (!text) return json({ error: "conflict" }, 409);
    const counterpart =
      body.profileId === row.sender_profile
        ? profile(row.recipient_profile).name
        : profile(row.sender_profile).name;
    const reading = interpret(normalizeDocument(text), body.profileId, { source, counterpart });
    return json({ interpretation: reading, stored: false, simulated: true });
  }

  private async history(request: Request, id: string): Promise<Response> {
    const auth = await this.authorize(request, id, "read");
    if ("error" in auth && auth.error) {
      const status = auth.error === "unauthorized" ? 401 : auth.error === "forbidden" ? 403 : 404;
      return json({ error: auth.error }, status);
    }
    const row = auth.row;
    if (!row) return json({ error: "not_found" }, 404);
    const history = await this.log(row.repo_name);
    return json({ ...this.coordination(row.repo_name), history });
  }

  private async reset(request: Request): Promise<Response> {
    const expected = this.env.DEMO_RESET_TOKEN ?? "";
    const provided = request.headers.get("X-Demo-Reset") ?? "";
    if (!tokenEquals(provided, expected)) return json({ error: "not_found" }, 404);
    const listed = this.all("SELECT id, repo_name FROM handoffs");
    for (const item of listed) await this.removeRepo(String(item.repo_name));
    this.sql("DELETE FROM events");
    this.sql("DELETE FROM invitations");
    this.sql("DELETE FROM sessions");
    this.sql("DELETE FROM handoffs");
    return json({ reset: true, handoffs: listed.length });
  }

  private async authorize(
    request: Request,
    id: string,
    action: Action,
  ): Promise<{ row?: HandoffRow; role: Role; error?: string }> {
    const token = bearer(request);
    const session = token ? await this.sessionByToken(token) : null;
    const row = this.handoff(id);
    const decision = decideAccess({
      handoffExists: Boolean(row),
      requestedId: id,
      session: session
        ? { handoffId: session.handoff_id, role: session.role, expiresAt: session.expires_at }
        : null,
      action,
      now: Date.now(),
    });
    if (decision !== "ok") return { role: session?.role ?? "sender", error: decision };
    if (!row || !session) return { role: "sender", error: "not_found" };
    if (decideTransition(row.status, action) !== "ok") return { role: session.role, error: "conflict" };
    return { row, role: session.role };
  }

  private async view(row: HandoffRow, role: Role) {
    const original = await this.readPath(row.repo_name, "original.md");
    const interpretation = await this.readJsonFile<Interpretation>(row.repo_name, "interpretation.json");
    const response = await this.readPath(row.repo_name, "response.md");
    const returnInterpretation = await this.readJsonFile<Interpretation>(row.repo_name, "return.json");
    const questions = parseQuestions(await this.readPath(row.repo_name, "questions.md"));
    const history = await this.log(row.repo_name);
    const sha = original ? await sha256Hex(normalizeDocument(original)) : "";
    const invite = role === "sender" ? this.activeInvite(row.id) : null;
    return {
      id: row.id,
      title: row.title,
      status: row.status,
      you: role,
      sender: profile(row.sender_profile),
      recipient: profile(row.recipient_profile),
      originalMarkdown: original ? normalizeDocument(original) : "",
      interpretation,
      questions,
      responseMarkdown: response ? normalizeDocument(response) : null,
      returnInterpretation,
      history,
      integrity: sha === row.original_sha ? "sealed" : "mismatch",
      originalCommit: row.original_commit,
      headCommit: row.head_commit,
      invitation: invite,
      coordination: this.coordination(row.repo_name),
      events: this.events(row.id),
    };
  }

  private async commit(repoName: string, files: Record<string, string>, message: string): Promise<string> {
    if ("original.md" in files) throw new Error("original is sealed");
    if (!this.env.ARTIFACTS) return this.localSave(repoName, files, message, false);
    const creds = await this.writeToken(repoName);
    return pushUpdate(creds.remote, creds.token, files, message);
  }

  private async localSave(
    repoName: string,
    files: Record<string, string>,
    message: string,
    replace: boolean,
  ): Promise<string> {
    if (replace) {
      this.sql("DELETE FROM documents WHERE repo_name = ?", repoName);
    }
    const previous = this.one(
      "SELECT hash FROM revisions WHERE repo_name = ? ORDER BY id DESC LIMIT 1",
      repoName,
    );
    const material = `${previous?.hash ?? ""}\n${message}\n${Object.keys(files)
      .sort()
      .map((path) => `${path}\n${files[path]}`)
      .join("\n")}`;
    const hash = await sha256Hex(material);
    for (const [path, content] of Object.entries(files)) {
      this.sql(
        `INSERT INTO documents (repo_name, path, content) VALUES (?, ?, ?)
         ON CONFLICT(repo_name, path) DO UPDATE SET content = excluded.content`,
        repoName,
        path,
        content,
      );
    }
    this.sql(
      "INSERT INTO revisions (repo_name, hash, message, created_at) VALUES (?, ?, ?, ?)",
      repoName,
      hash,
      message,
      new Date().toISOString(),
    );
    return hash;
  }

  private coordination(repo: string) {
    const artifacts = Boolean(this.env.ARTIFACTS);
    return {
      service: "cloudflare-workers" as const,
      persistence: artifacts ? ("cloudflare-artifacts" as const) : ("cloudflare-durable-object" as const),
      namespace: NAMESPACE,
      repo,
      simulatedAgent: true as const,
      note: artifacts
        ? "The original and each accepted revision are commits in Cloudflare Artifacts. Access checks run in this Worker."
        : "Cloudflare Artifacts is not enabled on this account. The Worker stores the sealed original and each revision in its Durable Object.",
    };
  }

  private async writeToken(repoName: string): Promise<{ remote: string; token: string }> {
    const artifacts = this.env.ARTIFACTS;
    if (!artifacts) throw new Error("artifacts binding missing");
    using repo = await artifacts.get(repoName);
    const info = await repo.info();
    const minted = await repo.createToken("write", 600);
    if (!info.remote || !minted.plaintext) throw new Error("artifact credentials missing");
    return { remote: info.remote, token: minted.plaintext };
  }

  private async currentFiles(repoName: string): Promise<Record<string, string>> {
    if (!this.env.ARTIFACTS) {
      const files: Record<string, string> = {};
      for (const row of this.all("SELECT path, content FROM documents WHERE repo_name = ?", repoName)) {
        files[String(row.path)] = String(row.content);
      }
      if (!files["original.md"]) throw new Error("original missing from store");
      return files;
    }
    using repo = await this.env.ARTIFACTS.get(repoName);
    const files: Record<string, string> = {};
    for (const path of FILE_PATHS) {
      const text = await readText(repo, path);
      if (text !== null) files[path] = text.endsWith("\n") ? text : `${text}\n`;
    }
    if (!files["original.md"]) throw new Error("original missing from artifact");
    return files;
  }

  private async assertOriginal(row: HandoffRow): Promise<void> {
    const text = await this.readRequired(row.repo_name, "original.md");
    const sha = await sha256Hex(normalizeDocument(text));
    if (sha !== row.original_sha) throw new Error("original changed after write");
  }

  private async readPath(repoName: string, path: string): Promise<string | null> {
    if (!this.env.ARTIFACTS) {
      const row = this.one(
        "SELECT content FROM documents WHERE repo_name = ? AND path = ?",
        repoName,
        path,
      );
      return row ? String(row.content) : null;
    }
    using repo = await this.env.ARTIFACTS.get(repoName);
    return readText(repo, path);
  }

  private async readRequired(repoName: string, path: string): Promise<string> {
    let last: unknown;
    for (let attempt = 0; attempt < 6; attempt += 1) {
      try {
        const text = await this.readPath(repoName, path);
        if (text !== null) return text;
      } catch (error) {
        last = error;
      }
      await sleep(300);
    }
    throw last instanceof Error ? last : new Error("artifact file not ready");
  }

  private async readJsonFile<T>(repoName: string, path: string): Promise<T | null> {
    const text = await this.readPath(repoName, path);
    if (!text) return null;
    try {
      return JSON.parse(text) as T;
    } catch {
      return null;
    }
  }

  private async log(repoName: string): Promise<HistoryEntry[]> {
    if (!this.env.ARTIFACTS) {
      return this.all(
        "SELECT hash, message FROM revisions WHERE repo_name = ? ORDER BY id DESC LIMIT 20",
        repoName,
      ).map((row) => ({ hash: String(row.hash), message: String(row.message) }));
    }
    using repo = await this.env.ARTIFACTS.get(repoName);
    const commits = await repo.log({ ref: "main", limit: 20 });
    return historyFromLog(commits);
  }

  private async removeRepo(repoName: string): Promise<void> {
    this.sql("DELETE FROM documents WHERE repo_name = ?", repoName);
    this.sql("DELETE FROM revisions WHERE repo_name = ?", repoName);
    if (!this.env.ARTIFACTS) return;
    await this.env.ARTIFACTS.delete(repoName).catch((error) => console.error(scrub(error)));
  }

  private async saveSession(token: string, handoffId: string, role: Role): Promise<void> {
    const hash = await sha256Hex(token);
    this.sql(
      "INSERT INTO sessions (token_hash, handoff_id, role, expires_at) VALUES (?, ?, ?, ?)",
      hash,
      handoffId,
      role,
      Date.now() + SESSION_MS,
    );
  }

  private async sessionByToken(token: string): Promise<SessionRow | null> {
    const hash = await sha256Hex(token);
    const row = this.one(
      "SELECT token_hash, handoff_id, role, expires_at FROM sessions WHERE token_hash = ?",
      hash,
    );
    if (!row) return null;
    const role = row.role === "recipient" ? "recipient" : "sender";
    return {
      token_hash: String(row.token_hash),
      handoff_id: String(row.handoff_id),
      role,
      expires_at: Number(row.expires_at),
    };
  }

  private handoff(id: string): HandoffRow | null {
    const row = this.one("SELECT * FROM handoffs WHERE id = ?", id);
    if (!row || !isProfileId(row.sender_profile) || !isProfileId(row.recipient_profile)) return null;
    const status = String(row.status);
    if (!isStatus(status)) return null;
    return {
      id: String(row.id),
      repo_name: String(row.repo_name),
      title: String(row.title),
      status,
      sender_profile: row.sender_profile,
      recipient_profile: row.recipient_profile,
      original_sha: String(row.original_sha),
      original_commit: row.original_commit ? String(row.original_commit) : null,
      head_commit: row.head_commit ? String(row.head_commit) : null,
      created_at: String(row.created_at),
      updated_at: String(row.updated_at),
    };
  }

  private activeInvite(handoffId: string): { active: boolean; expiresAt: string | null } {
    const row = this.one(
      "SELECT expires_at FROM invitations WHERE handoff_id = ? AND spent = 0 AND revoked = 0 AND expires_at > ? ORDER BY expires_at DESC LIMIT 1",
      handoffId,
      Date.now(),
    );
    if (!row) return { active: false, expiresAt: null };
    return { active: true, expiresAt: new Date(Number(row.expires_at)).toISOString() };
  }

  private addEvent(
    handoffId: string,
    kind: string,
    message: string,
    commitHash: string | null,
    createdAt: string,
  ) {
    this.sql(
      "INSERT INTO events (handoff_id, kind, message, commit_hash, created_at) VALUES (?, ?, ?, ?, ?)",
      handoffId,
      kind,
      message,
      commitHash,
      createdAt,
    );
  }

  private events(handoffId: string) {
    return this.all(
      "SELECT kind, message, commit_hash, created_at FROM events WHERE handoff_id = ? ORDER BY id ASC",
      handoffId,
    ).map((row) => ({
      kind: String(row.kind),
      message: String(row.message),
      commitHash: row.commit_hash ? String(row.commit_hash) : null,
      createdAt: String(row.created_at),
    }));
  }

  private sql(query: string, ...args: unknown[]): void {
    this.ctx.storage.sql.exec(query, ...args);
  }

  private one(query: string, ...args: unknown[]): Row | null {
    const list = this.ctx.storage.sql.exec(query, ...args).toArray() as Row[];
    return list[0] ?? null;
  }

  private all(query: string, ...args: unknown[]): Row[] {
    return this.ctx.storage.sql.exec(query, ...args).toArray() as Row[];
  }
}

function isStatus(value: string): value is HandoffStatus {
  return ["sealed", "invited", "interpreted", "returned", "acknowledged"].includes(value);
}

function tokenEquals(left: string, right: string): boolean {
  if (!left || !right || left.length !== right.length) return false;
  let diff = 0;
  for (let index = 0; index < left.length; index += 1) {
    diff |= left.charCodeAt(index) ^ right.charCodeAt(index);
  }
  return diff === 0;
}
