import type { Host, Id } from "../../ipc/api";
import type { ProtocolFilter } from "../../stores/hosts";

export interface HostFilter {
  filter: ProtocolFilter;
  query: string;
  /** `null` = all groups. */
  groupId: Id | null;
}

/** Every whitespace separated term must occur in name, address, user or a tag (case-insensitive). */
export function matchesQuery(host: Host, query: string): boolean {
  const terms = query.toLowerCase().split(/\s+/).filter((term) => term.length > 0);
  if (terms.length === 0) return true;
  const fields = [host.name, host.address, host.user ?? "", ...host.tags].map((field) => field.toLowerCase());
  return terms.every((term) => fields.some((field) => field.includes(term)));
}

export function filterHosts(hosts: readonly Host[], { filter, query, groupId }: HostFilter): Host[] {
  return hosts.filter(
    (host) =>
      (filter === "all" || (filter === "ssh" ? host.protocolSsh : host.protocolSftp)) &&
      (groupId === null || host.groupId === groupId) &&
      matchesQuery(host, query),
  );
}
