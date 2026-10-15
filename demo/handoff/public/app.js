const STATUS = {
  sealed: "Original sealed",
  invited: "Invitation open",
  interpreted: "Interpreted for the recipient",
  returned: "Return handoff sent",
  acknowledged: "Return acknowledged",
};

const LABELS = {
  wants: "What they need understood",
  decisions: "Decisions",
  actions: "Action items",
  assumptions: "Assumptions",
  dependencies: "Dependencies",
  questions: "Questions and ambiguities",
  scope: "Scope",
};

const app = document.querySelector("#app");
let poll;
let pendingInvite = "";
let lastStamp = "";

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "className") node.className = value;
    else if (key.startsWith("on") && typeof value === "function") node.addEventListener(key.slice(2).toLowerCase(), value);
    else if (value !== undefined && value !== null) node.setAttribute(key, String(value));
  }
  for (const child of children) node.append(child);
  return node;
}

function text(value) {
  return document.createTextNode(value ?? "");
}

function storageKey(id) {
  return `ce-personal-handoff:${id}`;
}

function sessions(id) {
  try {
    return JSON.parse(sessionStorage.getItem(storageKey(id)) || "{}");
  } catch {
    return {};
  }
}

function saveSessions(id, value) {
  sessionStorage.setItem(storageKey(id), JSON.stringify(value));
}

async function api(path, options = {}) {
  const headers = {};
  if (options.token) headers.authorization = `Bearer ${options.token}`;
  if (options.body) headers["content-type"] = "application/json";
  const response = await fetch(path, {
    method: options.method || "GET",
    headers,
    body: options.body ? JSON.stringify(options.body) : undefined,
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(payload.error || "request_failed");
    error.payload = payload;
    throw error;
  }
  return payload;
}

function handoffId() {
  const match = /^\/h\/(h[0-9a-f]{24})$/.exec(location.pathname);
  return match ? match[1] : "";
}

function renderMarkdown(target, markdown) {
  target.replaceChildren();
  let list = null;
  for (const line of String(markdown || "").split("\n")) {
    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading) {
      list = null;
      target.append(el(heading[1].length === 1 ? "h3" : "h4", {}, [text(heading[2])]));
      continue;
    }
    const bullet = /^[-*]\s+(\[[ xX]\]\s+)?(.+)$/.exec(line);
    if (bullet) {
      if (!list) {
        list = el("ul");
        target.append(list);
      }
      list.append(el("li", {}, [text(`${bullet[1] || ""}${bullet[2]}`)]));
      continue;
    }
    if (!line.trim()) {
      list = null;
      continue;
    }
    list = null;
    target.append(el("p", {}, [text(line)]));
  }
}

function interpretationView(reading, preview) {
  const box = el("div", { className: "split" });
  if (!reading) {
    box.append(el("p", { className: "muted" }, [text("The receiving agent has not opened this handoff yet.")]));
    return box;
  }
  if (preview) {
    box.append(el("div", { className: "banner small" }, [text("Preview only. This profile is not stored. The original markdown stays sealed.")]));
  }
  box.append(el("p", {}, [text(reading.executiveSummary)]));
  box.append(el("p", { className: "small muted" }, [text(reading.lens)]));
  box.append(el("p", { className: "small" }, [text(reading.boundary)]));
  for (const key of reading.displayOrder || []) {
    const items = reading[key] || [];
    if (!items.length) continue;
    const card = el("div", { className: "split" }, [el("h3", {}, [text(LABELS[key] || key)])]);
    const list = el("ul");
    for (const item of items) list.append(el("li", {}, [text(item)]));
    card.append(list);
    box.append(card);
  }
  return box;
}

async function boot() {
  clearInterval(poll);
  const id = handoffId();
  if (!id) {
    await renderHome();
    return;
  }
  await renderRoom(id);
  poll = setInterval(() => {
    if (document.visibilityState === "visible") renderRoom(id, true);
  }, 3000);
}

async function renderHome() {
  const sample = await api("/api/sample");
  const profiles = (await api("/api/profiles")).profiles;
  app.replaceChildren();
  const sender = el("select");
  const recipient = el("select");
  for (const profile of profiles) {
    sender.append(el("option", { value: profile.id }, [text(`${profile.name} — ${profile.role}`)]));
    recipient.append(el("option", { value: profile.id }, [text(`${profile.name} — ${profile.role}`)]));
  }
  sender.value = "avery";
  recipient.value = "jordan";
  const markdown = el("textarea");
  markdown.value = sample.markdown;
  const error = el("p", { className: "error small hidden" });
  const button = el("button", { type: "button" }, [text("Create handoff")]);
  button.addEventListener("click", async () => {
    button.disabled = true;
    error.classList.add("hidden");
    try {
      const created = await api("/api/handoffs", {
        method: "POST",
        body: {
          markdown: markdown.value,
          senderProfileId: sender.value,
          recipientProfileId: recipient.value,
        },
      });
      saveSessions(created.handoff.id, { sender: created.sessionToken, role: "sender" });
      history.pushState({}, "", `/h/${created.handoff.id}`);
      await boot();
    } catch (cause) {
      error.textContent = cause.message === "profiles_must_differ"
        ? "Choose two different profiles. The demo shows two people."
        : "The handoff could not be created. Check the document and try again.";
      error.classList.remove("hidden");
      button.disabled = false;
    }
  });

  app.append(
    el("header", { className: "top" }, [
      el("div", {}, [
        el("div", { className: "brand" }, [text("Complexity Eater")]),
        el("h1", {}, [text("Different minds. Shared work.")]),
        el("p", { className: "lede" }, [text("Paste the markdown your agent already wrote. Invite one person. Their agent reads only this handoff and turns it into the work they need. The original stays sealed.")]),
      ]),
    ]),
    el("div", { className: "banner" }, [text("Simulated agents. Avery, Jordan, and Riley are demo profiles. Their readings are deterministic and do not call a model provider.")]),
    el("div", { className: "grid", style: "margin-top:1rem" }, [
      el("section", { className: "card stack" }, [
        el("h2", {}, [text("Start a handoff")]),
        el("label", {}, [text("Sender"), sender]),
        el("label", {}, [text("Recipient"), recipient]),
        el("label", {}, [text("Markdown from the sender's agent"), markdown]),
        el("div", { className: "row" }, [
          button,
          el("button", {
            type: "button",
            className: "secondary",
            onclick: () => { markdown.value = sample.markdown; },
          }, [text("Reset sample")]),
        ]),
        error,
      ]),
      el("aside", { className: "card stack" }, [
        el("h2", {}, [text("What the recipient gets")]),
        el("p", {}, [text("A short summary, the decision or the work, the open questions, and a draft reply in their own working style.")]),
        el("p", { className: "muted small" }, [text("The invitation is one use, expires in 60 minutes, and can be revoked. The code travels in the link fragment, which the browser does not send as a query string.")]),
        el("div", { className: "meta small" }, [text("Coordination runs on one Cloudflare Worker. Document revisions are stored with that Worker. Artifacts commits are used when the account binding is enabled.")]),
      ]),
    ]),
  );
}

async function renderRoom(id, quiet) {
  const stored = sessions(id);
  const hash = new URLSearchParams(location.hash.replace(/^#/, ""));
  const invite = hash.get("invite");
  if (invite) {
    history.replaceState({}, "", location.pathname);
    try {
      const redeemed = await api("/api/invitations/redeem", {
        method: "POST",
        body: { handoffId: id, code: invite },
      });
      stored.recipient = redeemed.sessionToken;
      stored.role = "recipient";
      saveSessions(id, stored);
    } catch {
      paintMessage(id, "This invitation is unavailable. Ask the sender for a new one.");
      return;
    }
  }
  const role = stored.role === "recipient" && stored.recipient ? "recipient" : "sender";
  const token = role === "recipient" ? stored.recipient : stored.sender;
  if (!token) {
    paintMessage(id, "This handoff is private. Open it from the browser that created it, or use an invitation link.");
    return;
  }
  let payload;
  try {
    payload = await api(`/api/handoffs/${id}`, { token });
  } catch {
    if (!quiet) paintMessage(id, "This session cannot read the handoff. Redeem a fresh invitation or start again.");
    return;
  }
  const handoff = payload.handoff;
  const stamp = `${handoff.you}|${handoff.status}|${handoff.headCommit}|${(handoff.questions || []).length}`;
  if (quiet && stamp === lastStamp) return;
  lastStamp = stamp;
  paintRoom(handoff, token, stored);
}

function paintMessage(id, message) {
  app.replaceChildren(
    el("header", { className: "top" }, [
      el("div", {}, [
        el("div", { className: "brand" }, [text("Complexity Eater")]),
        el("h1", {}, [text("Personal handoff")]),
        el("p", {}, [text(message)]),
      ]),
    ]),
    el("a", { href: "/", className: "button secondary" }, [text("Start a handoff")]),
  );
  document.querySelector("a.button")?.addEventListener("click", (event) => {
    event.preventDefault();
    history.pushState({}, "", "/");
    boot();
  });
}

function paintRoom(handoff, token, stored) {
  lastStamp = `${handoff.you}|${handoff.status}|${handoff.headCommit}|${(handoff.questions || []).length}`;
  const focus = document.activeElement;
  const typing = focus && (focus.tagName === "TEXTAREA" || focus.tagName === "INPUT" || focus.tagName === "SELECT");
  if (typing) return;

  app.replaceChildren();
  const you = handoff.you;
  const other = you === "sender" ? handoff.recipient : handoff.sender;
  const error = el("p", { className: "error small hidden" });
  const original = el("div", { className: "markdown" });
  renderMarkdown(original, handoff.originalMarkdown);
  const readingHost = el("div");
  if (you === "recipient") {
    readingHost.append(interpretationView(handoff.interpretation, false));
  } else if (handoff.interpretation) {
    readingHost.append(
      el("p", { className: "small muted" }, [text(`Stored reading for ${handoff.recipient.name}. It stays separate from the original.`)]),
      interpretationView(handoff.interpretation, false),
    );
  } else {
    readingHost.append(el("p", { className: "muted" }, [text(`${handoff.recipient.name}'s agent has not opened this yet. The original is already sealed.`)]));
  }

  const preview = el("select");
  for (const id of ["jordan", "avery", "riley"]) {
    const person = id === handoff.sender.id ? handoff.sender : id === handoff.recipient.id ? handoff.recipient : null;
    const label = person ? person.name : id === "riley" ? "Riley Okonkwo" : id;
    preview.append(el("option", { value: id }, [text(label)]));
  }
  preview.value = handoff.recipient.id;
  preview.addEventListener("change", async () => {
    if (preview.value === handoff.recipient.id) {
      readingHost.replaceChildren(interpretationView(handoff.interpretation, false));
      return;
    }
    const result = await api(`/api/handoffs/${handoff.id}/preview`, {
      method: "POST",
      token,
      body: { profileId: preview.value, source: "original" },
    });
    readingHost.replaceChildren(interpretationView(result.interpretation, true));
  });

  const responseBox = el("textarea");
  responseBox.value = handoff.responseMarkdown || handoff.interpretation?.suggestedResponse || "";
  const question = el("input", { type: "text", placeholder: "Ask one question about the handoff" });

  async function run(path, body, nextRole) {
    error.classList.add("hidden");
    try {
      const result = await api(path, { method: "POST", token, body });
      if (result.inviteCode) pendingInvite = `${location.origin}/h/${handoff.id}#invite=${result.inviteCode}`;
      if (path.endsWith("/revoke")) pendingInvite = "";
      if (result.handoff) paintRoom(result.handoff, token, stored);
      if (nextRole) boot();
    } catch (cause) {
      error.textContent = cause.message === "conflict"
        ? "That step is not available in the current handoff state."
        : "The request was not saved.";
      error.classList.remove("hidden");
    }
  }

  const actions = el("div", { className: "row" });
  if (you === "sender") {
    actions.append(el("button", { type: "button", onclick: () => run(`/api/handoffs/${handoff.id}/invitations`) }, [text(handoff.invitation?.active ? "Replace invitation" : "Create invitation")]));
    if (handoff.invitation?.active) {
      actions.append(el("button", { type: "button", className: "secondary", onclick: () => run(`/api/handoffs/${handoff.id}/invitations/revoke`) }, [text("Revoke invitation")]));
    }
    if (handoff.status === "returned") {
      actions.append(el("button", { type: "button", onclick: () => run(`/api/handoffs/${handoff.id}/acknowledge`) }, [text("Acknowledge return")]));
    }
  } else {
    actions.append(el("button", { type: "button", onclick: () => run(`/api/handoffs/${handoff.id}/response`, { markdown: responseBox.value }) }, [text("Send response back")]));
    actions.append(el("button", { type: "button", className: "secondary", onclick: () => run(`/api/handoffs/${handoff.id}/questions`, { text: question.value }) }, [text("Add question")]));
  }

  const inviteField = el("input", { id: "invite-link", type: "text", readonly: "readonly" });
  if (pendingInvite) inviteField.value = pendingInvite;
  const inviteRow = el("div", { className: pendingInvite ? "stack" : "stack hidden" }, [
    el("p", { className: "small" }, [text("Give this link to the recipient. Open it in a private window. The code is in the fragment, so it is not a query parameter. It works once.")]),
    inviteField,
    el("button", { type: "button", className: "secondary", onclick: async () => { await navigator.clipboard.writeText(inviteField.value); } }, [text("Copy invitation")]),
  ]);

  const history = el("ul", { className: "history" });
  for (const commit of handoff.history || []) {
    history.append(el("li", {}, [el("code", {}, [text((commit.hash || "").slice(0, 12) || "pending")]), text(` ${commit.message || "Artifact commit"}`)]));
  }
  const timeline = el("ul", { className: "history" });
  for (const event of handoff.events || []) {
    timeline.append(el("li", { className: "small" }, [text(event.message)]));
  }

  const returnPanel = el("div", { className: "stack" });
  if (handoff.responseMarkdown) {
    const responseView = el("div", { className: "markdown" });
    renderMarkdown(responseView, handoff.responseMarkdown);
    returnPanel.append(
      el("h3", {}, [text(`${handoff.recipient.name}'s response`)]),
      responseView,
      el("h3", {}, [text(`${handoff.sender.name}'s reading of the return`)]),
      interpretationView(handoff.returnInterpretation, false),
    );
  }

  const both = stored.sender && stored.recipient;
  const switcher = el("div", { className: "row" });
  if (both) {
    switcher.append(el("span", { className: "small muted" }, [text("This browser holds both sessions.")]));
    switcher.append(el("button", {
      type: "button",
      className: "secondary",
      onclick: () => {
        stored.role = you === "sender" ? "recipient" : "sender";
        saveSessions(handoff.id, stored);
        boot();
      },
    }, [text(you === "sender" ? `View as ${handoff.recipient.name}` : `View as ${handoff.sender.name}`)]));
  }

  app.append(
    el("header", { className: "top" }, [
      el("div", {}, [
        el("div", { className: "brand" }, [text("Complexity Eater · personal handoff")]),
        el("h1", {}, [text(handoff.title)]),
        el("p", { className: "muted" }, [text(`${handoff.sender.name}, ${handoff.sender.role} → ${handoff.recipient.name}, ${handoff.recipient.role}`)]),
      ]),
      el("div", { className: "stack actions" }, [
        el("div", { className: "pill" }, [text(STATUS[handoff.status] || handoff.status)]),
        el("a", { href: "/", className: "button secondary" }, [text("New handoff")]),
      ]),
    ]),
    el("div", { className: "banner" }, [text(`Simulated agent. You are ${you === "sender" ? handoff.sender.name : handoff.recipient.name}. This reading is mock output, separate from the original markdown.`)]),
    el("div", { className: handoff.integrity === "sealed" ? "meta" : "banner", style: "margin-top:0.8rem" }, [
      text(`${handoff.integrity === "sealed" ? "Original sealed" : "Original mismatch"}. ${handoff.coordination.note} Store ${handoff.coordination.repo}.`),
    ]),
    switcher,
    el("div", { className: "room", style: "margin-top:1rem" }, [
      el("section", { className: "card" }, [
        el("h2", {}, [text("Original markdown")]),
        el("p", { className: "small muted" }, [text(`From ${handoff.sender.name}'s agent. Commit ${(handoff.originalCommit || "").slice(0, 12)}.`)]),
        original,
      ]),
      el("section", { className: "card stack" }, [
        el("h2", {}, [text(you === "sender" ? `${handoff.recipient.name}'s reading` : `${handoff.recipient.name}'s agent`)]),
        you === "recipient" ? el("label", {}, [text("Read the same original as"), preview]) : el("p", { className: "small muted" }, [text("The sender does not receive the recipient's private profile. They see the return when it is sent.")]),
        readingHost,
      ]),
    ]),
    handoff.responseMarkdown ? el("section", { className: "card", style: "margin-top:1rem" }, [el("h2", {}, [text("Return handoff")]), returnPanel]) : el("span"),
    el("section", { className: "card stack", style: "margin-top:1rem" }, [
      el("h2", {}, [text(you === "sender" ? "Invite and acknowledge" : "Question or response")]),
      you === "recipient" ? question : el("span"),
      you === "recipient" ? responseBox : el("span"),
      actions,
      inviteRow,
      error,
      handoff.questions?.length ? el("div", {}, [el("h3", {}, [text("Questions on the handoff")]), ...handoff.questions.map((item) => el("p", {}, [text(item)]))]) : el("span"),
    ]),
    el("section", { className: "card", style: "margin-top:1rem" }, [
      el("h2", {}, [text(handoff.coordination.persistence === "cloudflare-artifacts" ? "Artifact history" : "Revision history")]),
      el("p", { className: "small muted" }, [text(handoff.coordination.persistence === "cloudflare-artifacts"
        ? "Each line is a commit read back from Cloudflare Artifacts. Later revisions do not rewrite the original file."
        : "Each line is a revision stored in the Worker Durable Object. Later revisions do not rewrite the original file. This is not an Artifacts commit.")]),
      history,
      el("h3", {}, [text("Room timeline")]),
      timeline,
    ]),
  );
  document.querySelector('a.secondary')?.addEventListener("click", (event) => {
    if (event.currentTarget.getAttribute("href") !== "/") return;
    event.preventDefault();
    history.pushState({}, "", "/");
    boot();
  });
}

window.addEventListener("popstate", () => boot());
boot();
