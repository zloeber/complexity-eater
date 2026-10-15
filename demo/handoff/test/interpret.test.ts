import { describe, expect, it } from "vitest";
import { interpret } from "../src/interpret";
import { SAMPLE_MARKDOWN } from "../src/sample";

const counterpart = "Avery Chen";

describe("recipient interpretation", () => {
  const jordan = interpret(SAMPLE_MARKDOWN, "jordan", { source: "original", counterpart });
  const avery = interpret(SAMPLE_MARKDOWN, "avery", { source: "original", counterpart: "Jordan Hale" });
  const riley = interpret(SAMPLE_MARKDOWN, "riley", { source: "original", counterpart });

  it("marks every reading as a simulated mock agent", () => {
    for (const reading of [jordan, avery, riley]) {
      expect(reading.simulated).toBe(true);
      expect(reading.agent).toBe("mock");
      expect(reading.boundary).toContain("does not open the other person's agent memory");
    }
  });

  it("extracts the same work and orders it for each profile", () => {
    expect(jordan.actions).toHaveLength(4);
    expect(jordan.decisions).toHaveLength(3);
    expect(jordan.questions).toHaveLength(3);
    expect(jordan.assumptions).toHaveLength(3);
    expect(jordan.dependencies).toHaveLength(3);
    expect(jordan.scope).toHaveLength(3);
    expect(jordan.displayOrder[0]).toBe("actions");
    expect(avery.displayOrder[0]).toBe("decisions");
    expect(riley.displayOrder[0]).toBe("scope");
    expect(jordan.executiveSummary).not.toBe(avery.executiveSummary);
    expect(riley.executiveSummary).not.toBe(jordan.executiveSummary);
  });

  it("keeps script-looking text as data", () => {
    const reading = interpret("- <script>alert(1)</script>\n", "jordan", {
      source: "original",
      counterpart,
    });
    expect(reading.actions[0]).toBe("<script>alert(1)</script>");
    expect(JSON.stringify(reading)).not.toContain("\"html\"");
  });

  it("reads a return handoff for the sender's date-first profile", () => {
    const reading = interpret("We can decide the retry budget.\n\nNo calendar was attached.\n", "avery", {
      source: "response",
      counterpart: "Jordan Hale",
    });
    expect(reading.executiveSummary).toContain("No date is stated");
    expect(reading.source).toBe("response");
  });
});
