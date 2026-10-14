import { describe, expect, it } from "vitest";
import { makeHost } from "../../test/fixtures";
import { filterHosts, matchesQuery } from "./filterHosts";

const web = makeHost({ id: "web", name: "web-prod-01", tags: ["nginx", "docker"], groupId: "g-prod" });
const db = makeHost({ id: "db", name: "db-prod-01", address: "10.0.1.20", user: "postgres", protocolSftp: false, groupId: "g-prod" });
const shop = makeHost({ id: "shop", name: "kunde-shop", address: "sftp.kunde-a.example", user: "shop", protocolSsh: false });
const all = [web, db, shop];
const ids = (hosts: { id: string }[]) => hosts.map((h) => h.id);

describe("matchesQuery", () => {
  it("matches name, address, user and tags case-insensitively", () => {
    expect(matchesQuery(web, "WEB")).toBe(true);
    expect(matchesQuery(db, "10.0.1.20")).toBe(true);
    expect(matchesQuery(db, "postgres")).toBe(true);
    expect(matchesQuery(web, "docker")).toBe(true);
    expect(matchesQuery(web, "")).toBe(true);
  });

  it("requires every whitespace separated term to match", () => {
    expect(matchesQuery(web, "prod nginx")).toBe(true);
    expect(matchesQuery(web, "prod postgres")).toBe(false);
  });
});

describe("filterHosts", () => {
  it("filters by protocol", () => {
    expect(ids(filterHosts(all, { filter: "all", query: "", groupId: null }))).toEqual(["web", "db", "shop"]);
    expect(ids(filterHosts(all, { filter: "ssh", query: "", groupId: null }))).toEqual(["web", "db"]);
    expect(ids(filterHosts(all, { filter: "sftp", query: "", groupId: null }))).toEqual(["web", "shop"]);
  });

  it("combines group, protocol and query", () => {
    expect(ids(filterHosts(all, { filter: "all", query: "", groupId: "g-prod" }))).toEqual(["web", "db"]);
    expect(ids(filterHosts(all, { filter: "sftp", query: "", groupId: "g-prod" }))).toEqual(["web"]);
    expect(ids(filterHosts(all, { filter: "all", query: "kunde", groupId: null }))).toEqual(["shop"]);
  });
});
