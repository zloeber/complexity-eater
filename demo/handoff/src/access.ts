export type Role = "sender" | "recipient";

export type Action =
  | "read"
  | "invite"
  | "revoke"
  | "question"
  | "respond"
  | "acknowledge"
  | "preview";

export type Access = "ok" | "unauthorized" | "forbidden" | "not_found";

const SENDER_ACTIONS: Action[] = ["read", "invite", "revoke", "acknowledge", "preview"];
const RECIPIENT_ACTIONS: Action[] = ["read", "question", "respond", "preview"];

export function decideAccess(input: {
  handoffExists: boolean;
  requestedId: string;
  session: { handoffId: string; role: Role; expiresAt: number } | null;
  action: Action;
  now: number;
}): Access {
  const session = input.session;
  if (!session || session.expiresAt <= input.now) return "unauthorized";
  if (!input.handoffExists || session.handoffId !== input.requestedId) return "not_found";
  const allowed = session.role === "sender" ? SENDER_ACTIONS : RECIPIENT_ACTIONS;
  return allowed.includes(input.action) ? "ok" : "forbidden";
}

export type HandoffStatus = "sealed" | "invited" | "interpreted" | "returned" | "acknowledged";

export function decideTransition(
  status: HandoffStatus,
  action: Action,
): "ok" | "conflict" {
  if (action === "question" || action === "respond") {
    return status === "interpreted" || status === "returned" ? "ok" : "conflict";
  }
  if (action === "acknowledge") return status === "returned" ? "ok" : "conflict";
  return "ok";
}

export type InviteFailure = "missing" | "expired" | "spent" | "revoked" | "mismatch";

export function decideRedeem(input: {
  invite: { handoffId: string; expiresAt: number; spent: boolean; revoked: boolean } | null;
  requestedHandoffId: string;
  now: number;
}): "ok" | InviteFailure {
  const invite = input.invite;
  if (!invite) return "missing";
  if (invite.handoffId !== input.requestedHandoffId) return "mismatch";
  if (invite.revoked) return "revoked";
  if (invite.spent) return "spent";
  if (invite.expiresAt <= input.now) return "expired";
  return "ok";
}

export function publicInviteError(_reason: InviteFailure): "invitation_unavailable" {
  return "invitation_unavailable";
}

const QUERY_SECRETS = ["invite", "code", "token", "session", "markdown", "authorization"];

export function hasCredentialQuery(url: URL): boolean {
  for (const key of url.searchParams.keys()) {
    if (QUERY_SECRETS.includes(key.toLowerCase())) return true;
  }
  return false;
}

export type Route =
  | { kind: "health" }
  | { kind: "profiles" }
  | { kind: "sample" }
  | { kind: "create" }
  | { kind: "redeem" }
  | { kind: "reset" }
  | { kind: "handoff"; id: string; action: Action | "history" };

const HANDOFF_PATH = /^\/api\/handoffs\/([A-Za-z0-9_-]{8,40})(?:\/([a-z/]+))?$/;

export function routeApi(method: string, pathname: string): Route | null {
  if (method === "GET" && pathname === "/api/health") return { kind: "health" };
  if (method === "GET" && pathname === "/api/profiles") return { kind: "profiles" };
  if (method === "GET" && pathname === "/api/sample") return { kind: "sample" };
  if (method === "POST" && pathname === "/api/handoffs") return { kind: "create" };
  if (method === "POST" && pathname === "/api/invitations/redeem") return { kind: "redeem" };
  if (method === "POST" && pathname === "/api/demo/reset") return { kind: "reset" };

  const match = HANDOFF_PATH.exec(pathname);
  if (!match) return null;
  const id = match[1];
  const tail = match[2] ?? "";
  if (!id) return null;
  if (method === "GET" && tail === "") return { kind: "handoff", id, action: "read" };
  if (method === "GET" && tail === "history") return { kind: "handoff", id, action: "history" };
  if (method === "POST" && tail === "invitations") return { kind: "handoff", id, action: "invite" };
  if (method === "POST" && tail === "invitations/revoke") {
    return { kind: "handoff", id, action: "revoke" };
  }
  if (method === "POST" && tail === "questions") return { kind: "handoff", id, action: "question" };
  if (method === "POST" && tail === "response") return { kind: "handoff", id, action: "respond" };
  if (method === "POST" && tail === "acknowledge") {
    return { kind: "handoff", id, action: "acknowledge" };
  }
  if (method === "POST" && tail === "preview") return { kind: "handoff", id, action: "preview" };
  return null;
}
