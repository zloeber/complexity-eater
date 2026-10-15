import { type Profile, type ProfileId, profile } from "./profiles";

export type Interpretation = {
  simulated: true;
  agent: "mock";
  profileId: ProfileId;
  profileName: string;
  source: "original" | "response";
  executiveSummary: string;
  lens: string;
  boundary: string;
  wants: string[];
  decisions: string[];
  actions: string[];
  assumptions: string[];
  dependencies: string[];
  questions: string[];
  scope: string[];
  displayOrder: Array<
    "wants" | "decisions" | "actions" | "assumptions" | "dependencies" | "questions" | "scope"
  >;
  suggestedResponse: string;
};

const ORDER: Record<ProfileId, Interpretation["displayOrder"]> = {
  avery: ["decisions", "questions", "wants", "actions", "dependencies", "assumptions", "scope"],
  jordan: ["actions", "questions", "dependencies", "wants", "assumptions", "decisions", "scope"],
  riley: ["scope", "assumptions", "questions", "wants", "actions", "decisions", "dependencies"],
};

type Kind = "decision" | "action" | "assumption" | "dependency" | "question" | "scope" | "wants" | "other";

function kindOf(title: string): Kind {
  const text = title.toLowerCase();
  if (/want/.test(text)) return "wants";
  if (/decision/.test(text)) return "decision";
  if (/action|task|to do|todo/.test(text)) return "action";
  if (/assum/.test(text)) return "assumption";
  if (/depend/.test(text)) return "dependency";
  if (/question|ambigu|unresolved/.test(text)) return "question";
  if (/scope/.test(text)) return "scope";
  return "other";
}

function bullets(body: string): string[] {
  return body
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /^[-*] /.test(line))
    .map((line) => line.replace(/^[-*] (\[[ xX]\] )?/, "").trim())
    .filter(Boolean);
}

function firstParagraph(markdown: string): string {
  const beforeHeading = markdown.split(/^#{1,3} /m)[0] ?? "";
  const paragraph = beforeHeading
    .split(/\n\s*\n/)
    .map((part) => part.replace(/\s+/g, " ").trim())
    .find((part) => part && !part.startsWith("#"));
  return paragraph ?? "";
}

function sections(markdown: string): Array<{ kind: Kind; title: string; body: string }> {
  const parts = markdown.split(/^(#{1,3} .*)$/m);
  const found: Array<{ kind: Kind; title: string; body: string }> = [];
  for (let index = 1; index < parts.length; index += 2) {
    const heading = parts[index] ?? "";
    const title = heading.replace(/^#{1,3}\s+/, "").trim();
    const body = parts[index + 1] ?? "";
    found.push({ kind: kindOf(title), title, body });
  }
  return found;
}

function unique(items: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const item of items) {
    const key = item.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(item);
  }
  return result;
}

function classifyLoose(line: string): Kind {
  const text = line.toLowerCase();
  if (text.includes("?")) return "question";
  if (/\b(decide|decision)\b/.test(text)) return "decision";
  if (/\b(depend|dependency|blocked by)\b/.test(text)) return "dependency";
  if (/\b(assum)/.test(text)) return "assumption";
  if (/\b(out of scope|not in scope)\b/.test(text)) return "scope";
  return "action";
}

function countLabel(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function mentionsDate(markdown: string): boolean {
  return /\b(20\d{2}|january|february|march|april|may|june|july|august|september|october|november|december|monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b/i.test(
    markdown,
  );
}

export function interpret(
  markdown: string,
  profileId: ProfileId,
  options: { source: "original" | "response"; counterpart: string },
): Interpretation {
  const reader = profile(profileId);
  const buckets: Record<Kind, string[]> = {
    decision: [],
    action: [],
    assumption: [],
    dependency: [],
    question: [],
    scope: [],
    wants: [],
    other: [],
  };

  for (const section of sections(markdown)) {
    for (const item of bullets(section.body)) {
      const kind = section.kind === "other" ? classifyLoose(item) : section.kind;
      buckets[kind].push(item);
    }
  }

  if (buckets.action.length + buckets.question.length + buckets.decision.length === 0) {
    for (const item of bullets(markdown)) {
      buckets[classifyLoose(item)].push(item);
    }
  }

  const decisions = unique(buckets.decision);
  const actions = unique(buckets.action);
  const assumptions = unique(buckets.assumption);
  const dependencies = unique(buckets.dependency);
  const questions = unique(buckets.question);
  const scope = unique(buckets.scope);
  const wants = unique(buckets.wants);
  const opening = firstParagraph(markdown);
  const summary = executiveSummary(reader.id, options.source, {
    decisions,
    actions,
    assumptions,
    dependencies,
    questions,
    scope,
    opening,
    dated: mentionsDate(markdown),
    counterpart: options.counterpart,
  });

  return {
    simulated: true,
    agent: "mock",
    profileId: reader.id,
    profileName: reader.name,
    source: options.source,
    executiveSummary: summary,
    lens: reader.style,
    boundary: `This reading uses the handoff text and ${reader.name}'s demo profile only. It does not open the other person's agent memory, tools, or files.`,
    wants,
    decisions,
    actions,
    assumptions,
    dependencies,
    questions,
    scope,
    displayOrder: ORDER[reader.id],
    suggestedResponse: suggestedResponse(reader, options.counterpart, {
      decisions,
      actions,
      dependencies,
      questions,
    }),
  };
}

function executiveSummary(
  profileId: ProfileId,
  source: "original" | "response",
  input: {
    decisions: string[];
    actions: string[];
    assumptions: string[];
    dependencies: string[];
    questions: string[];
    scope: string[];
    opening: string;
    dated: boolean;
    counterpart: string;
  },
): string {
  const lead = input.opening ? ` ${input.opening}` : "";
  if (profileId === "avery") {
    const dateNote = input.dated
      ? "A date is present in the text."
      : "No date is stated. Ask for one before treating this as closed.";
    return source === "response"
      ? `Avery, read the return for a decision and a date. ${countLabel(input.decisions.length, "decision")} and ${countLabel(input.questions.length, "open question")}. ${dateNote}`
      : `Avery, this is waiting on a decision and a date. ${countLabel(input.decisions.length, "decision")} to make, ${countLabel(input.questions.length, "question")} that can block it, and ${countLabel(input.actions.length, "listed action")}.${lead}`;
  }
  if (profileId === "riley") {
    return `Riley, check whether this expands the promise. ${countLabel(input.scope.length, "scope line")}, ${countLabel(input.assumptions.length, "assumption")}, and ${countLabel(input.actions.length, "requested action")}.${lead}`;
  }
  return `Jordan, make this concrete before you estimate. ${countLabel(input.actions.length, "action")}, ${countLabel(input.dependencies.length, "dependency")}, and ${countLabel(input.questions.length, "ambiguity")}. Vague alignment is not a task.${lead}`;
}

function suggestedResponse(
  reader: Profile,
  counterpart: string,
  input: {
    decisions: string[];
    actions: string[];
    dependencies: string[];
    questions: string[];
  },
): string {
  if (reader.id === "jordan") {
    const blockers = [...input.decisions.slice(0, 2), ...input.dependencies.slice(0, 1)];
    const lines = [
      `${counterpart}, I read this as concrete work rather than a status update.`,
      "",
      "I can take these actions once the blocking questions are closed:",
      ...(input.actions.length ? input.actions.map((item) => `- ${item}`) : ["- None were explicit in the handoff."]),
      "",
      "Questions I need answered before I estimate:",
      ...(input.questions.length
        ? input.questions.map((item) => `- ${item}`)
        : ["- The handoff did not list an explicit question."]),
      "",
      `I can name a decision on the open items by 16 October 2026. I am not estimating until these are explicit: ${
        blockers.join("; ") || "the questions above"
      }.`,
    ];
    return lines.join("\n");
  }
  if (reader.id === "riley") {
    return [
      `${counterpart}, I read this for what a user would be promised and what stays out of scope.`,
      "",
      "Please confirm the scope boundary before engineering treats the action list as committed.",
      ...(input.questions.slice(0, 3).map((item) => `- ${item}`)),
    ].join("\n");
  }
  const decision = input.decisions[0] ?? "the open decision";
  const question = input.questions[0] ?? "the blocking question";
  return [
    `${counterpart}, here is the return in the form I can use.`,
    "",
    `- Decision: ${decision}`,
    "- Date: 16 October 2026",
    `- Blocking question: ${question}`,
  ].join("\n");
}
