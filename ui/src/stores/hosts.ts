import { create } from "zustand";
import {
  type Group,
  groupDelete,
  groupSave,
  groupsList,
  type Host,
  hostDelete,
  hostSave,
  hostsList,
  type Id,
  type LoginProfile,
  type ProfileWithUsage,
  profileDelete,
  profileSave,
  profilesList,
  sshConfigReload,
} from "../ipc/api";
import { useSnippetsStore } from "./snippets";

export type ProtocolFilter = "all" | "ssh" | "sftp";

export interface HostsState {
  hosts: Host[];
  groups: Group[];
  profiles: ProfileWithUsage[];
  loaded: boolean;
  filter: ProtocolFilter;
  query: string;
  /** Sidebar group selection; `null` = "Alle Hosts". */
  groupId: Id | null;
  selectedId: Id | null;
  load(): Promise<void>;
  reloadSshConfig(): Promise<void>;
  saveHost(host: Host): Promise<Host>;
  deleteHost(id: Id): Promise<void>;
  saveProfile(profile: LoginProfile): Promise<LoginProfile>;
  deleteProfile(id: Id): Promise<void>;
  saveGroup(group: Group): Promise<Group>;
  deleteGroup(id: Id): Promise<void>;
  setFilter(filter: ProtocolFilter): void;
  setQuery(query: string): void;
  setGroupId(groupId: Id | null): void;
  select(id: Id | null): void;
}

export const useHostsStore = create<HostsState>()((set, get) => ({
  hosts: [],
  groups: [],
  profiles: [],
  loaded: false,
  filter: "all",
  query: "",
  groupId: null,
  selectedId: null,
  load: async () => {
    const [hosts, groups, profiles] = await Promise.all([hostsList(), groupsList(), profilesList()]);
    set({ hosts, groups, profiles, loaded: true });
  },
  reloadSshConfig: async () => {
    set({ hosts: await sshConfigReload() });
  },
  saveHost: async (host) => {
    const saved = await hostSave(host);
    await get().load();
    return saved;
  },
  deleteHost: async (id) => {
    await hostDelete(id);
    set((state) => ({ selectedId: state.selectedId === id ? null : state.selectedId }));
    // The host's snippets are gone too (ON DELETE CASCADE).
    await Promise.all([get().load(), useSnippetsStore.getState().load()]);
  },
  saveProfile: async (profile) => {
    const saved = await profileSave(profile);
    await get().load();
    return saved;
  },
  deleteProfile: async (id) => {
    await profileDelete(id);
    await get().load();
  },
  saveGroup: async (group) => {
    const saved = await groupSave(group);
    await get().load();
    return saved;
  },
  deleteGroup: async (id) => {
    await groupDelete(id);
    set((state) => ({ groupId: state.groupId === id ? null : state.groupId }));
    await get().load();
  },
  setFilter: (filter) => set({ filter }),
  setQuery: (query) => set({ query }),
  setGroupId: (groupId) => set({ groupId }),
  select: (selectedId) => set({ selectedId }),
}));
