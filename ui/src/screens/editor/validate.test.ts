import { describe, expect, it } from "vitest";
import { makeHost, makeProfile } from "../../test/fixtures";
import { hostToDraft, NEW_GROUP, profileToDraft } from "./drafts";
import { envError, errorTabs, hasErrors, isNonNegativeInt, validateHost, validatePort, validateProfile } from "./validate";

const valid = hostToDraft(makeHost());

describe("field rules", () => {
  it("accepts ports 1–65535 only", () => {
    expect(validatePort("22")).toBeUndefined();
    expect(validatePort("65535")).toBeUndefined();
    for (const port of ["0", "65536", "", "2a", "-1", "1.5"]) expect(validatePort(port), port).toBe("Port muss zwischen 1 und 65535 liegen");
  });

  it("accepts non-negative integers only", () => {
    expect(isNonNegativeInt("0")).toBe(true);
    expect(isNonNegativeInt(" 30 ")).toBe(true);
    expect(isNonNegativeInt("-1")).toBe(false);
    expect(isNonNegativeInt("1.5")).toBe(false);
    expect(isNonNegativeInt("")).toBe(false);
  });

  it("requires KEY=VALUE env lines", () => {
    expect(envError("LANG=C\n\nA_1=")).toBeUndefined();
    expect(envError("LANG=C\nkaputt")).toBe("Zeile 2: Format KEY=VALUE");
    expect(envError("1X=a")).toBe("Zeile 1: Format KEY=VALUE");
  });
});

describe("validateHost", () => {
  it("accepts a valid draft", () => {
    expect(validateHost(valid)).toEqual({});
    expect(hasErrors(validateHost(valid))).toBe(false);
  });

  it("reports every invalid field", () => {
    const errors = validateHost({
      ...valid,
      name: " ",
      address: "",
      port: "0",
      groupId: NEW_GROUP,
      newGroupName: "",
      keepalive: "x",
      timeout: "-5",
      env: "nope",
      forwards: [{ id: "f", kind: "local", bind: "8080", target: "", autostart: false }],
      sftp: { ...valid.sftp, parallel: "", bandwidthLimit: "fast" },
    });
    expect(errors).toEqual({
      name: "Pflichtfeld",
      address: "Pflichtfeld",
      port: "Port muss zwischen 1 und 65535 liegen",
      newGroupName: "Gruppennamen angeben",
      keepalive: "Ganze Zahl ≥ 0 eingeben",
      timeout: "Ganze Zahl ≥ 0 eingeben",
      env: "Zeile 1: Format KEY=VALUE",
      forwards: "Jede Regel braucht Bind-Adresse und Ziel",
      parallel: "Ganze Zahl ≥ 0 eingeben",
      bandwidthLimit: "Ganze Zahl ≥ 0 eingeben",
    });
    expect(errorTabs(errors)).toEqual(["general", "connection", "sftp"]);
  });

  it("checks auth: profile selection, key path, address without spaces", () => {
    expect(validateHost({ ...valid, authSource: "profile", profileId: "" })).toEqual({ profileId: "Login-Profil auswählen" });
    expect(validateHost({ ...valid, auth: { ...valid.auth, method: "key", keyPath: "" } })).toEqual({
      keyPath: "Pfad zum privaten Schlüssel angeben",
    });
    expect(validateHost({ ...valid, authSource: "profile", profileId: "p", auth: { ...valid.auth, method: "key", keyPath: "" } })).toEqual({});
    expect(validateHost({ ...valid, address: "a b" })).toEqual({ address: "Die Adresse darf keine Leerzeichen enthalten" });
    expect(validateHost({ ...valid, forwards: [{ id: "f", kind: "dynamic", bind: "1080", target: "", autostart: false }] })).toEqual({});
  });
});

describe("validateProfile", () => {
  it("requires name, user and a key path for key auth", () => {
    const draft = profileToDraft(makeProfile());
    expect(validateProfile(draft)).toEqual({});
    expect(validateProfile({ ...draft, name: "", user: "", auth: { ...draft.auth, keyPath: "" } })).toEqual({
      name: "Pflichtfeld",
      user: "Pflichtfeld",
      keyPath: "Pfad zum privaten Schlüssel angeben",
    });
  });
});
