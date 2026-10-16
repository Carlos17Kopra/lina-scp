/**
 * Typed wrappers around the generated tauri-specta bindings.
 * Every function resolves with the data or rejects with a `CoreError` (see `./errors`).
 * Tests mock this module: `vi.mock("<relative>/ipc/api")` and `vi.mocked(api.fn).mockResolvedValue(…)`.
 */
import { Channel } from "@tauri-apps/api/core";
import { commands } from "./bindings";
import type {
  Bookmark,
  Difference,
  SyncDeleteReport,
  SyncItem,
  PaneSide,
  ChmodReport,
  ChosenApp,
  CancelTarget,
  Decision,
  Direction,
  ConnectedSession,
  ConnectEvent,
  CoreError,
  ExportReport,
  Group,
  Host,
  ImportPreview,
  ImportReport,
  Listing,
  LoginProfile,
  ModeChange,
  OpenApp,
  ProfileWithUsage,
  PromptAnswer,
  Resolution,
  SecretKind,
  SecretOwner,
  SessionEventDto,
  Settings,
  SftpConnected,
  SftpEvent,
  Snippet,
} from "./bindings";

export type {
  ChosenApp,
  OpenApp,
  Bookmark,
  EditEvent,
  EditFile,
  EditState,
  CancelTarget,
  Decision,
  Direction,
  FileStamp,
  Job,
  JobKind,
  JobNote,
  JobState,
  TransferEvent,
  ConnectedSession,
  AuthConfig,
  AuthMethod,
  Conflict,
  ConnectEvent,
  CoreError,
  Entry,
  Difference,
  DiffStatus,
  NodeInfo,
  DirContents,
  TreeEntry,
  PaneSide,
  SyncDeleteReport,
  SyncItem,
  PathFailure,
  ChmodReport,
  EntryKind,
  EnvVar,
  ExportReport,
  ForwardKind,
  FsKind,
  Group,
  Host,
  HostAuth,
  HostKeyDecision,
  HostKeyInfo,
  HostSource,
  ImportItem,
  ImportPreview,
  ImportReport,
  ItemKind,
  KbdPrompt,
  LinkTarget,
  Listing,
  LoginProfile,
  ModeChange,
  NetworkKind,
  PortForward,
  ProfileWithUsage,
  Prompt,
  PromptAnswer,
  Resolution,
  SecretKind,
  SecretOwner,
  SessionEventDto,
  SessionInfo,
  Settings,
  SftpConnected,
  SftpEvent,
  SftpSettings,
  Snippet,
  SshMode,
  Step,
  TerminalRenderer,
  Theme,
} from "./bindings";

/** UUID string (hosts, profiles, groups, connect attempts). */
export type Id = string;
export type SessionId = string;

type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: CoreError };

async function unwrap<T>(pending: Promise<CommandResult<T>>): Promise<T> {
  const result = await pending;
  if (result.status === "error") {
    throw result.error;
  }
  return result.data;
}

async function unwrapVoid(pending: Promise<CommandResult<null>>): Promise<void> {
  await unwrap(pending);
}

/** New random UUID for hosts, profiles, groups and connect attempts. */
export function newId(): Id {
  return crypto.randomUUID();
}

// Hosts & groups
export function hostsList(): Promise<Host[]> {
  return unwrap(commands.hostsList());
}
export function hostSave(host: Host): Promise<Host> {
  return unwrap(commands.hostSave(host));
}
export function hostDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.hostDelete(id));
}
export function groupsList(): Promise<Group[]> {
  return unwrap(commands.groupsList());
}
export function groupSave(group: Group): Promise<Group> {
  return unwrap(commands.groupSave(group));
}
export function groupDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.groupDelete(id));
}

// Login profiles & secrets
export function profilesList(): Promise<ProfileWithUsage[]> {
  return unwrap(commands.profilesList());
}
export function profileSave(profile: LoginProfile): Promise<LoginProfile> {
  return unwrap(commands.profileSave(profile));
}
export function profileDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.profileDelete(id));
}
export function secretSet(owner: SecretOwner, kind: SecretKind, value: string): Promise<void> {
  return unwrapVoid(commands.secretSet(owner, kind, value));
}
export function secretDelete(owner: SecretOwner, kind: SecretKind): Promise<void> {
  return unwrapVoid(commands.secretDelete(owner, kind));
}

// Bookmarks (hostId null = local)
export function bookmarksList(hostId: Id | null): Promise<Bookmark[]> {
  return unwrap(commands.bookmarksList(hostId));
}
export function bookmarkAdd(hostId: Id | null, path: string): Promise<Bookmark> {
  return unwrap(commands.bookmarkAdd(hostId, path));
}
export function bookmarkRemove(id: Id): Promise<void> {
  return unwrapVoid(commands.bookmarkRemove(id));
}

// Snippets
export function snippetsList(): Promise<Snippet[]> {
  return unwrap(commands.snippetsList());
}
export function snippetSave(snippet: Snippet): Promise<Snippet> {
  return unwrap(commands.snippetSave(snippet));
}
export function snippetDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.snippetDelete(id));
}

// Settings
export function settingsGet(): Promise<Settings> {
  return unwrap(commands.settingsGet());
}
export function settingsSet(settings: Settings): Promise<Settings> {
  return unwrap(commands.settingsSet(settings));
}

// Connect pipeline
export function connectSsh(
  connectId: Id,
  hostId: Id,
  cols: number,
  rows: number,
  events: Channel<ConnectEvent>,
  output: Channel<ArrayBuffer>,
  sessionEvents: Channel<SessionEventDto>,
): Promise<ConnectedSession> {
  return unwrap(commands.connectSsh(connectId, hostId, cols, rows, events, output, sessionEvents));
}
/** Resolves `false` when the prompt was already answered, timed out or cancelled. */
export function connectAnswer(promptId: number, answer: PromptAnswer): Promise<boolean> {
  return unwrap(commands.connectAnswer(promptId, answer));
}
export function connectCancel(connectId: Id): Promise<void> {
  return unwrapVoid(commands.connectCancel(connectId));
}
/** "Verbindung testen": runs until Auth for an (unsaved) host; cancel with `connectCancel(connectId)`. */
export function connectTest(connectId: Id, host: Host, events: Channel<ConnectEvent>): Promise<void> {
  return unwrapVoid(commands.connectTest(connectId, host, events));
}

// SFTP commander
export function connectSftp(
  connectId: string,
  hostId: Id,
  sessionId: SessionId | null,
  events: Channel<ConnectEvent>,
  sftpEvents: Channel<SftpEvent>,
): Promise<SftpConnected> {
  return unwrap(commands.connectSftp(connectId, hostId, sessionId, events, sftpEvents));
}
export function transferEnqueue(sessionId: SessionId, direction: Direction, sources: string[], targetDir: string): Promise<number> {
  return unwrap(commands.transferEnqueue(sessionId, direction, sources, targetDir));
}
export function transferResolve(sessionId: SessionId, jobId: number, decision: Decision, applyToAll: boolean): Promise<void> {
  return unwrapVoid(commands.transferResolve(sessionId, jobId, decision, applyToAll));
}
export function transferCancel(sessionId: SessionId, target: CancelTarget): Promise<void> {
  return unwrapVoid(commands.transferCancel(sessionId, target));
}
export function transferRetry(sessionId: SessionId, jobId: number): Promise<void> {
  return unwrapVoid(commands.transferRetry(sessionId, jobId));
}
export function transferClear(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.transferClear(sessionId));
}
export function sftpReconnectNow(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sftpReconnectNow(sessionId));
}
export function sftpStopReconnect(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sftpStopReconnect(sessionId));
}
export function sftpList(sessionId: SessionId, path: string): Promise<Listing> {
  return unwrap(commands.sftpList(sessionId, path));
}
export function sftpMkdir(sessionId: SessionId, dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.sftpMkdir(sessionId, dir, name));
}
export function sftpRename(sessionId: SessionId, dir: string, from: string, to: string): Promise<void> {
  return unwrapVoid(commands.sftpRename(sessionId, dir, from, to));
}
export function sftpDelete(sessionId: SessionId, dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.sftpDelete(sessionId, dir, names));
}
/** Resolves with the edited copies that were kept (they hold changes the server does not have). */
export function sftpClose(sessionId: SessionId): Promise<string[]> {
  return unwrap(commands.sftpClose(sessionId));
}
export function sftpChmod(sessionId: SessionId, paths: string[], change: ModeChange, recursive: boolean): Promise<ChmodReport> {
  return unwrap(commands.sftpChmod(sessionId, paths, change, recursive));
}
export function syncCompare(sessionId: SessionId, localDir: string, remoteDir: string, exclude: string[]): Promise<Difference[]> {
  return unwrap(commands.syncCompare(sessionId, localDir, remoteDir, exclude));
}
export function syncCancel(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.syncCancel(sessionId));
}
export function syncEnqueue(sessionId: SessionId, direction: Direction, localDir: string, remoteDir: string, items: SyncItem[], exclude: string[]): Promise<number[]> {
  return unwrap(commands.syncEnqueue(sessionId, direction, localDir, remoteDir, items, exclude));
}
export function syncDelete(sessionId: SessionId, side: PaneSide, baseDir: string, items: SyncItem[], exclude: string[]): Promise<SyncDeleteReport> {
  return unwrap(commands.syncDelete(sessionId, side, baseDir, items, exclude));
}
export function sftpCreateFile(sessionId: SessionId, dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.sftpCreateFile(sessionId, dir, name));
}
export function editOpen(sessionId: SessionId, path: string, how: OpenApp): Promise<number> {
  return unwrap(commands.editOpen(sessionId, path, how));
}
export function localEdit(path: string, how: OpenApp): Promise<void> {
  return unwrapVoid(commands.localEdit(path, how));
}
/** The system's "Open with" dialog; resolves `null` when cancelled. */
export function openWithChoose(name: string, rememberLabel: string): Promise<ChosenApp | null> {
  return unwrap(commands.openWithChoose(name, rememberLabel));
}
export function editResolve(sessionId: SessionId, editId: number, overwrite: boolean): Promise<void> {
  return unwrapVoid(commands.editResolve(sessionId, editId, overwrite));
}
export function editReopen(sessionId: SessionId, editId: number): Promise<void> {
  return unwrapVoid(commands.editReopen(sessionId, editId));
}
/** Resolves with the kept copy's path when it holds changes the server does not have. */
export function editStop(sessionId: SessionId, editId: number): Promise<string | null> {
  return unwrap(commands.editStop(sessionId, editId));
}
export function localList(path: string): Promise<Listing> {
  return unwrap(commands.localList(path));
}
export function localHome(hostId: Id | null): Promise<string> {
  return unwrap(commands.localHome(hostId));
}
export function localMkdir(dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.localMkdir(dir, name));
}
export function localCreateFile(dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.localCreateFile(dir, name));
}
export function localChmod(paths: string[], change: ModeChange, recursive: boolean): Promise<ChmodReport> {
  return unwrap(commands.localChmod(paths, change, recursive));
}
export function localRename(dir: string, from: string, to: string): Promise<void> {
  return unwrapVoid(commands.localRename(dir, from, to));
}
export function localDelete(dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.localDelete(dir, names));
}
export function localTrash(dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.localTrash(dir, names));
}

// Sessions
export function sessionOpenLocal(
  cols: number,
  rows: number,
  output: Channel<ArrayBuffer>,
  sessionEvents: Channel<SessionEventDto>,
): Promise<SessionId> {
  return unwrap(commands.sessionOpenLocal(cols, rows, output, sessionEvents));
}

const encoder = new TextEncoder();

/** Input typed while a session's write is in flight, sent as one batch afterwards. */
interface WriteBatch {
  chunks: Uint8Array[];
  done: Promise<void>;
  resolve: () => void;
  reject: (error: unknown) => void;
}

/** Key present = a write of that session is in flight; value = the batch waiting behind it. */
const writeQueues = new Map<SessionId, WriteBatch | undefined>();

async function sendInput(sessionId: SessionId, chunks: Uint8Array[]): Promise<void> {
  try {
    await unwrapVoid(commands.sessionWrite(sessionId, chunks.flatMap((chunk) => Array.from(chunk))));
  } finally {
    const next = writeQueues.get(sessionId);
    if (next) {
      writeQueues.set(sessionId, undefined);
      sendInput(sessionId, next.chunks).then(next.resolve, next.reject);
    } else {
      writeQueues.delete(sessionId);
    }
  }
}

/**
 * Accepts xterm `onData` strings (UTF-8 encoded here) or raw bytes. Tauri runs invokes
 * concurrently, so separate writes could overtake each other: per session only one write is in
 * flight, and input arriving meanwhile is batched behind it in order.
 */
export function sessionWrite(sessionId: SessionId, data: string | Uint8Array): Promise<void> {
  const bytes = typeof data === "string" ? encoder.encode(data) : data;
  if (!writeQueues.has(sessionId)) {
    writeQueues.set(sessionId, undefined);
    return sendInput(sessionId, [bytes]);
  }
  let batch = writeQueues.get(sessionId);
  if (!batch) {
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const done = new Promise<void>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    batch = { chunks: [], done, resolve, reject };
    writeQueues.set(sessionId, batch);
  }
  batch.chunks.push(bytes);
  return batch.done;
}
export function sessionResize(sessionId: SessionId, cols: number, rows: number): Promise<void> {
  return unwrapVoid(commands.sessionResize(sessionId, cols, rows));
}
/** Flow control: report bytes consumed by xterm (call from the `term.write` callback). */
export function sessionAck(sessionId: SessionId, bytes: number): Promise<void> {
  return unwrapVoid(commands.sessionAck(sessionId, bytes));
}
export function sessionClose(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionClose(sessionId));
}
/** Skip the reconnect wait, or start a new round after giving up (status `lost`). */
export function sessionReconnectNow(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionReconnectNow(sessionId));
}
/** Stop reconnecting (the pane shows "Verbindung verloren"). */
export function sessionReconnectStop(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionReconnectStop(sessionId));
}

// Export / import / ssh-config
export function exportFile(
  path: string,
  includeSecrets: boolean,
  includeKeyFiles: boolean,
  passphrase: string | null,
): Promise<ExportReport> {
  return unwrap(commands.exportFile(path, includeSecrets, includeKeyFiles, passphrase));
}
export function importPreview(path: string): Promise<ImportPreview> {
  return unwrap(commands.importPreview(path));
}
export function importApply(
  path: string,
  passphrase: string | null,
  resolutions: Record<Id, Resolution>,
): Promise<ImportReport> {
  return unwrap(commands.importApply(path, passphrase, resolutions));
}
export function sshConfigReload(): Promise<Host[]> {
  return unwrap(commands.sshConfigReload());
}

// Channels
export function createEventChannel<T>(onEvent: (event: T) => void): Channel<T> {
  return new Channel<T>(onEvent);
}

/** Raw terminal output: the backend sends `InvokeResponseBody::Raw`, JS receives an `ArrayBuffer`. */
export function createOutputChannel(onData: (bytes: Uint8Array) => void): Channel<ArrayBuffer> {
  return new Channel<ArrayBuffer>((buffer) => onData(new Uint8Array(buffer)));
}

/**
 * Output channel that can be created before the terminal exists (connect screen) and before the
 * session id is known: chunks are buffered until `attach`, acks are summed until `bind`.
 */
export interface OutputStream {
  readonly channel: Channel<ArrayBuffer>;
  /** Delivers buffered chunks synchronously, then live ones. Returns a detach function. */
  attach(onData: (bytes: Uint8Array) => void): () => void;
  /** Sets the session id once `connectSsh`/`sessionOpenLocal` resolved; flushes pending acks. */
  bind(sessionId: SessionId): void;
  /** Report consumed bytes (xterm write callback). */
  ack(bytes: number): void;
  /** Local bytes for the terminal (e.g. the reconnect separator), in order with the output, never acked. */
  inject(bytes: Uint8Array): void;
}

export function createOutputStream(): OutputStream {
  let consumer: ((bytes: Uint8Array) => void) | null = null;
  let pending: Uint8Array[] = [];
  let sessionId: SessionId | null = null;
  let unsentAck = 0;
  let localBytes = 0;

  const flushAck = () => {
    if (sessionId === null || unsentAck === 0) return;
    const bytes = unsentAck;
    unsentAck = 0;
    // A failed ack must not be lost (the backend pauses output above 1 MiB unacked): retry with the next one.
    sessionAck(sessionId, bytes).catch(() => {
      unsentAck += bytes;
    });
  };

  const deliver = (bytes: Uint8Array) => {
    if (consumer) consumer(bytes);
    else pending.push(bytes);
  };
  const channel = createOutputChannel(deliver);

  return {
    channel,
    attach(onData) {
      consumer = onData;
      const buffered = pending;
      pending = [];
      for (const chunk of buffered) onData(chunk);
      return () => {
        if (consumer === onData) consumer = null;
      };
    },
    bind(id) {
      sessionId = id;
      flushAck();
    },
    ack(bytes) {
      // xterm acks everything it wrote; injected bytes never came from the backend.
      const local = Math.min(localBytes, bytes);
      localBytes -= local;
      unsentAck += bytes - local;
      flushAck();
    },
    inject(bytes) {
      localBytes += bytes.length;
      deliver(bytes);
    },
  };
}
