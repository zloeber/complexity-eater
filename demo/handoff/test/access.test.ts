import { describe, expect, it } from "vitest";
import {
  decideAccess,
  decideRedeem,
  decideTransition,
  hasCredentialQuery,
  publicInviteError,
  routeApi,
  type Action,
  type InviteFailure,
} from "../src/access";
import { appendQuestion, initialFiles, sealUpdate } from "../src/files";
import { historyFromLog } from "../src/git-store";

const session = { handoffId: "habc", role: "sender" as const, expiresAt: 2_000 };
const now = 1_000;

describe("access", () => {
  it("rejects a missing or expired session before it looks up another handoff", () => {
    expect(
      decideAccess({
        handoffExists: true,
        requestedId: "habc",
        session: null,
        action: "read",
        now,
      }),
    ).toBe("unauthorized");
    expect(
      decideAccess({
        handoffExists: false,
        requestedId: "missing",
        session: null,
        action: "read",
        now,
      }),
    ).toBe("unauthorized");
    expect(
      decideAccess({
        handoffExists: true,
        requestedId: "habc",
        session: { ...session, expiresAt: 500 },
        action: "read",
        now,
      }),
    ).toBe("unauthorized");
  });

  it("hides other handoffs from a valid session", () => {
    expect(
      decideAccess({
        handoffExists: true,
        requestedId: "hother",
        session,
        action: "read",
        now,
      }),
    ).toBe("not_found");
  });

  it("keeps invite and response on opposite roles", () => {
    const sender: Action[] = ["invite", "revoke", "acknowledge"];
    const recipient: Action[] = ["question", "respond"];
    for (const action of sender) {
      expect(decideAccess({ handoffExists: true, requestedId: "habc", session, action, now })).toBe("ok");
      expect(
        decideAccess({
          handoffExists: true,
          requestedId: "habc",
          session: { ...session, role: "recipient" },
          action,
          now,
        }),
      ).toBe("forbidden");
    }
    for (const action of recipient) {
      expect(decideAccess({ handoffExists: true, requestedId: "habc", session, action, now })).toBe("forbidden");
      expect(
        decideAccess({
          handoffExists: true,
          requestedId: "habc",
          session: { ...session, role: "recipient" },
          action,
          now,
        }),
      ).toBe("ok");
    }
  });

  it("allows a response only after the recipient agent has interpreted the handoff", () => {
    expect(decideTransition("sealed", "respond")).toBe("conflict");
    expect(decideTransition("invited", "question")).toBe("conflict");
    expect(decideTransition("interpreted", "respond")).toBe("ok");
    expect(decideTransition("returned", "acknowledge")).toBe("ok");
    expect(decideTransition("acknowledged", "acknowledge")).toBe("conflict");
  });
});

describe("invitations", () => {
  const reasons: InviteFailure[] = ["missing", "expired", "spent", "revoked", "mismatch"];

  it("publishes one error for every failed redemption", () => {
    for (const reason of reasons) expect(publicInviteError(reason)).toBe("invitation_unavailable");
  });

  it("accepts a live unused invitation for the named handoff", () => {
    expect(
      decideRedeem({
        invite: { handoffId: "habc", expiresAt: 5_000, spent: false, revoked: false },
        requestedHandoffId: "habc",
        now: 1_000,
      }),
    ).toBe("ok");
    expect(
      decideRedeem({
        invite: { handoffId: "habc", expiresAt: 5_000, spent: true, revoked: false },
        requestedHandoffId: "habc",
        now: 1_000,
      }),
    ).toBe("spent");
  });

  it("rejects credentials carried in a query string", () => {
    expect(hasCredentialQuery(new URL("https://demo.example/h/abc?invite=secret"))).toBe(true);
    expect(hasCredentialQuery(new URL("https://demo.example/h/abc"))).toBe(false);
    expect(routeApi("POST", "/api/invitations/redeem")).toEqual({ kind: "redeem" });
    expect(routeApi("POST", "/api/handoffs/h123/original")).toBeNull();
  });
});

describe("sealed original", () => {
  it("writes the original once and refuses a later patch", () => {
    const files = initialFiles({
      handoffId: "h1",
      title: "Proposal",
      senderProfileId: "avery",
      recipientProfileId: "jordan",
      markdown: "# Proposal\n\nKeep this.\n",
      updatedAt: "2026-10-14T00:00:00.000Z",
    });
    expect(files["original.md"]).toContain("Keep this.");
    const next = sealUpdate(files, {
      "questions.md": appendQuestion(undefined, "Which clock?"),
    });
    expect(next["original.md"]).toBe(files["original.md"]);
    expect(() => sealUpdate(files, { "original.md": "# Replaced\n" })).toThrow(/sealed/);
  });
});

describe("artifact history", () => {
  it("reads commit hash and message without assuming one metadata shape", () => {
    expect(
      historyFromLog([
        { hash: "abc", message: "Seal original handoff\n\nbody" },
        { oid: "def", summary: "Record return handoff" },
      ]),
    ).toEqual([
      { hash: "abc", message: "Seal original handoff" },
      { hash: "def", message: "Record return handoff" },
    ]);
  });
});
