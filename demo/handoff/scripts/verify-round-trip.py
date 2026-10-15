#!/usr/bin/env python3
"""Run the personal-handoff round trip against a deployed or local Worker."""

import json
import sys
import urllib.error
import urllib.request

BASE = sys.argv[1] if len(sys.argv) > 1 else "https://complexity-eater-handoff.zloeber.workers.dev"


def call(method, path, body=None, token=None):
    data = None if body is None else json.dumps(body).encode()
    headers = {"user-agent": "Mozilla/5.0"}
    if body is not None:
        headers["content-type"] = "application/json"
    if token:
        headers["authorization"] = f"Bearer {token}"
    request = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request) as response:
            payload = response.read().decode()
            return response.status, json.loads(payload) if payload else {}
    except urllib.error.HTTPError as error:
        payload = error.read().decode()
        try:
            parsed = json.loads(payload) if payload else {}
        except json.JSONDecodeError:
            parsed = {"raw": payload}
        return error.code, parsed


def expect(status, body, code, label):
    if status != code:
        raise SystemExit(f"{label}: expected {code}, got {status} {body}")
    print(f"ok {label}")


health_status, health = call("GET", "/api/health")
expect(health_status, health, 200, "health")
if health.get("coordination") != "cloudflare-workers":
    raise SystemExit(f"health coordination: {health}")
print(f"persistence {health.get('persistence')}")

sample_status, sample = call("GET", "/api/sample")
expect(sample_status, sample, 200, "sample")
markdown = sample["markdown"]

status, created = call(
    "POST",
    "/api/handoffs",
    {"senderProfileId": "avery", "recipientProfileId": "jordan", "markdown": markdown},
)
expect(status, created, 201, "create")
handoff = created["handoff"]
sender = created["sessionToken"]
original = handoff["originalMarkdown"]
if handoff["integrity"] != "sealed":
    raise SystemExit(f"original was not sealed: {handoff['integrity']}")
if handoff["coordination"]["simulatedAgent"] is not True:
    raise SystemExit("missing simulated flag")
print(f"store {handoff['coordination']['persistence']} repo {handoff['coordination']['repo']}")

status, denied = call("GET", f"/api/handoffs/{handoff['id']}")
expect(status, denied, 401, "anonymous read")

status, query = call("GET", f"/api/handoffs/{handoff['id']}?token=not-a-real-secret")
expect(status, query, 400, "query credential rejected")

status, same = call(
    "POST",
    "/api/handoffs",
    {"senderProfileId": "avery", "recipientProfileId": "avery", "markdown": markdown},
)
expect(status, same, 400, "same profile rejected")

status, sender_view = call("GET", f"/api/handoffs/{handoff['id']}", token=sender)
expect(status, sender_view, 200, "sender read")
if sender_view["handoff"]["interpretation"] is not None:
    raise SystemExit("interpretation existed before the recipient opened the handoff")

status, invited = call("POST", f"/api/handoffs/{handoff['id']}/invitations", token=sender)
expect(status, invited, 200, "invite")
code = invited["inviteCode"]
if "invite=" in invited.get("handoffPath", ""):
    raise SystemExit("invite code leaked into the path")

status, early = call(
    "POST",
    f"/api/handoffs/{handoff['id']}/response",
    {"markdown": "too soon"},
    token=sender,
)
expect(status, early, 403, "sender cannot respond")

status, redeemed = call("POST", "/api/invitations/redeem", {"handoffId": handoff["id"], "code": code})
expect(status, redeemed, 200, "redeem")
recipient = redeemed["sessionToken"]
reading = redeemed["handoff"]["interpretation"]
if not reading or reading.get("simulated") is not True or reading.get("agent") != "mock":
    raise SystemExit(f"interpretation was not marked simulated: {reading}")
if reading["profileId"] != "jordan":
    raise SystemExit("recipient profile was not used")
if reading["displayOrder"][0] != "actions":
    raise SystemExit("Jordan's reading did not lead with actions")
if redeemed["handoff"]["originalMarkdown"] != original:
    raise SystemExit("original changed when the interpretation was stored")

status, again = call("POST", "/api/invitations/redeem", {"handoffId": handoff["id"], "code": code})
expect(status, again, 401, "invite spent")
if again.get("error") != "invitation_unavailable":
    raise SystemExit(f"spent invite error: {again}")

status, preview = call(
    "POST",
    f"/api/handoffs/{handoff['id']}/preview",
    {"profileId": "riley", "source": "original"},
    token=recipient,
)
expect(status, preview, 200, "preview")
if preview.get("stored") is not False or preview["interpretation"]["profileId"] != "riley":
    raise SystemExit(f"preview was stored or used the wrong profile: {preview}")
if preview["interpretation"]["displayOrder"][0] != "scope":
    raise SystemExit("Riley's preview did not lead with scope")

status, after_preview = call("GET", f"/api/handoffs/{handoff['id']}", token=recipient)
expect(status, after_preview, 200, "read after preview")
if after_preview["handoff"]["interpretation"]["profileId"] != "jordan":
    raise SystemExit("preview replaced the stored interpretation")
if after_preview["handoff"]["originalMarkdown"] != original:
    raise SystemExit("preview changed the original")

status, asked = call(
    "POST",
    f"/api/handoffs/{handoff['id']}/questions",
    {"text": "Does finance accept a 10 percent rollout?"},
    token=recipient,
)
expect(status, asked, 200, "question")
if asked["handoff"]["originalMarkdown"] != original:
    raise SystemExit("question changed the original")
if "10 percent rollout" not in " ".join(asked["handoff"]["questions"]):
    raise SystemExit("question was not stored")

response = reading["suggestedResponse"]
status, returned = call(
    "POST",
    f"/api/handoffs/{handoff['id']}/response",
    {"markdown": response},
    token=recipient,
)
expect(status, returned, 200, "response")
if returned["handoff"]["status"] != "returned":
    raise SystemExit(f"status after response: {returned['handoff']['status']}")
if returned["handoff"]["originalMarkdown"] != original:
    raise SystemExit("response changed the original")
return_reading = returned["handoff"]["returnInterpretation"]
if not return_reading or return_reading["profileId"] != "avery" or return_reading["source"] != "response":
    raise SystemExit(f"return reading: {return_reading}")

status, sender_return = call("GET", f"/api/handoffs/{handoff['id']}", token=sender)
expect(status, sender_return, 200, "sender sees return")
if sender_return["handoff"]["responseMarkdown"] is None:
    raise SystemExit("sender did not receive the response")
if sender_return["handoff"]["integrity"] != "sealed":
    raise SystemExit("integrity lost after the round trip")

status, ack = call("POST", f"/api/handoffs/{handoff['id']}/acknowledge", token=sender)
expect(status, ack, 200, "acknowledge")
if ack["handoff"]["status"] != "acknowledged":
    raise SystemExit(f"ack status: {ack['handoff']['status']}")
if ack["handoff"]["originalMarkdown"] != original:
    raise SystemExit("acknowledgement changed the original")

messages = [item["message"] for item in ack["handoff"]["history"]]
for expected_message in (
    "Seal original handoff",
    "Record recipient interpretation",
    "Record return handoff",
):
    if expected_message not in messages:
        raise SystemExit(f"missing revision {expected_message}: {messages}")

print("round trip complete")
print(f"url {BASE}/h/{handoff['id']}")
