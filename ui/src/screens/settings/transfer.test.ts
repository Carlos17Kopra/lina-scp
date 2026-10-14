import { describe, expect, it } from "vitest";
import { defaultResolutions, ensureExtension, exportFileName, isWrongPassphrase, reportMessage, validateExport } from "./transfer";

describe("export helpers", () => {
  it("names export files by date and enforces the extension", () => {
    expect(exportFileName(new Date(2026, 9, 11))).toBe("linascp-2026-10-11.linascp");
    expect(ensureExtension("/home/u/backup")).toBe("/home/u/backup.linascp");
    expect(ensureExtension("/home/u/backup.linascp")).toBe("/home/u/backup.linascp");
  });

  it("requires a repeated passphrase of at least 8 characters only with secrets", () => {
    expect(validateExport({ includeSecrets: false, passphrase: "", repeat: "" })).toEqual({});
    expect(validateExport({ includeSecrets: true, passphrase: "kurz", repeat: "kurz" })).toEqual({ passphrase: "Mindestens 8 Zeichen" });
    expect(validateExport({ includeSecrets: true, passphrase: "lang-genug", repeat: "lang-genu" })).toEqual({
      repeat: "Passphrasen stimmen nicht überein",
    });
    expect(validateExport({ includeSecrets: true, passphrase: "lang-genug", repeat: "lang-genug" })).toEqual({});
  });
});

describe("import helpers", () => {
  it("skips every conflict by default", () => {
    const preview = {
      hasSecrets: false,
      items: [
        { id: "h1", name: "web", kind: "host", conflict: "sameName" },
        { id: "h2", name: "db", kind: "host", conflict: null },
        { id: "p1", name: "deploy", kind: "profile", conflict: "sameId" },
      ],
    } as const;
    expect(defaultResolutions({ ...preview, items: [...preview.items] })).toEqual({ h1: "skip", p1: "skip" });
  });

  it("summarises the import report", () => {
    expect(reportMessage({ imported: 3, skipped: 1, secretsRestored: 0 })).toBe("Import abgeschlossen: 3 importiert, 1 übersprungen");
    expect(reportMessage({ imported: 1, skipped: 0, secretsRestored: 2 })).toBe(
      "Import abgeschlossen: 1 importiert, 0 übersprungen, 2 Geheimnisse wiederhergestellt",
    );
  });

  it("recognises a wrong passphrase", () => {
    expect(isWrongPassphrase({ type: "config", message: "wrong passphrase" })).toBe(true);
    expect(isWrongPassphrase({ type: "config", message: "unsupported format_version 2" })).toBe(false);
    expect(isWrongPassphrase(new Error("wrong passphrase"))).toBe(false);
  });
});
