import { describe, expect, it } from "vitest";
import { EMPTY_PROGRESS } from "../../stores/connectProgress";
import { errorTitle, stepItems, stepLabel, stepState } from "./steps";

describe("steps", () => {
  const progress = { ...EMPTY_PROGRESS, steps: ["jumpHost", "tcp", "auth"] as const, done: ["jumpHost" as const], current: "tcp" as const };

  it("labels steps in German", () => {
    expect(stepLabel("jumpHost", "bastion")).toBe("Jump-Host bastion verbinden");
    expect(stepLabel("hostKey")).toBe("Host-Key prüfen");
    expect(stepLabel("shell")).toBe("Shell starten");
  });

  it("derives step states and items with meta and detail", () => {
    const p = { ...progress, steps: [...progress.steps] };
    expect(stepState(p, "jumpHost")).toBe("done");
    expect(stepState(p, "tcp")).toBe("active");
    expect(stepState(p, "auth")).toBe("pending");
    expect(stepState({ ...p, failedStep: "auth" }, "auth")).toBe("failed");
    const items = stepItems(p, { jumpName: "bastion", meta: { tcp: "10.0.1.12:22", auth: "Profil deploy" }, detail: "prompt" });
    expect(items.map((i) => [i.label, i.state, i.meta, i.detail])).toEqual([
      ["Jump-Host bastion verbinden", "done", undefined, undefined],
      ["Verbindung herstellen", "active", "10.0.1.12:22", "prompt"],
      ["Anmelden", "pending", undefined, undefined],
    ]);
  });

  it("titles errors by category", () => {
    expect(errorTitle({ type: "network", kind: "timeout", message: "" })).toBe("Zeitüberschreitung");
    expect(errorTitle({ type: "network", kind: "other", message: "" })).toBe("Verbindung fehlgeschlagen");
    expect(errorTitle({ type: "auth", rejectedMethods: [], message: "" })).toBe("Anmeldung fehlgeschlagen");
    expect(errorTitle({ type: "cancelled" })).toBe("Abgebrochen");
    expect(errorTitle({ type: "config", message: "host x not found" })).toBe("host x not found");
  });
});
