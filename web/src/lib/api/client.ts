// Thin typed wrapper over fetch for the REST API. Types come from the generated schema.
import { takeEarly } from './early';
import type {
  ActionResult,
  AddRequest,
  AuthStatus,
  Category,
  Conflict,
  DiskInfo,
  DiskSpace,
  Disks,
  Credentials,
  Download,
  HostKeyProblem,
  KnownHost,
  PluginList,
  PluginView,
  SettingValue,
  Settings,
  StagedPlugin,
  Stats,
  StatsRange,
  StoreList,
  Torrent,
  TorrentFile,
  TorrentPeers,
  TrustHost,
  UrlInfo,
  VersionInfo,
} from './types';

export interface SpaceInfo {
  needed: number;
  free: number;
  reserved: number;
}

/** A failed API call. `status` is 0 when the server could not be reached at all. */
export class ApiError extends Error {
  readonly status: number;
  /** What the server said (`error` field), if anything. Not translated; shown as is. */
  readonly serverMessage: string | undefined;
  /** Set on `409` when a new download collides with an existing one. */
  readonly conflict: Conflict | undefined;
  /** Seconds until logins are allowed again (`429`). */
  readonly retryAfter: number | undefined;
  /** Set on `507` when a file does not fit: what it needs, what is free, and how much of that
   *  other unfinished downloads have reserved. */
  readonly space: SpaceInfo | undefined;
  /** Set on `409` when the host key of an SSH server (SFTP) is unknown or not the accepted one. */
  readonly hostKey: HostKeyProblem | undefined;

  constructor(
    status: number,
    serverMessage: string | undefined,
    extra: {
      conflict?: Conflict;
      retryAfter?: number;
      space?: SpaceInfo;
      hostKey?: HostKeyProblem;
    } = {},
  ) {
    // `message` is for logs and tests; the UI builds its own text (see `errorText`).
    super(serverMessage ?? `HTTP ${status}`);
    this.name = 'ApiError';
    this.status = status;
    this.serverMessage = serverMessage;
    this.conflict = extra.conflict;
    this.retryAfter = extra.retryAfter;
    this.space = extra.space;
    this.hostKey = extra.hostKey;
  }
}

let onUnauthorized: (() => void) | null = null;

/** Called when a protected endpoint answers 401 (session expired or server restarted). */
export function setUnauthorizedHandler(handler: (() => void) | null): void {
  onUnauthorized = handler;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null;
}

function parseError(status: number, body: unknown): ApiError {
  const message = isRecord(body) && typeof body.error === 'string' ? body.error : undefined;
  const conflict =
    isRecord(body) && isRecord(body.conflict) ? (body.conflict as Conflict) : undefined;
  const retryAfter =
    isRecord(body) && typeof body.retry_after === 'number' ? body.retry_after : undefined;
  const space =
    isRecord(body) &&
    typeof body.needed === 'number' &&
    typeof body.free === 'number' &&
    typeof body.reserved === 'number'
      ? { needed: body.needed, free: body.free, reserved: body.reserved }
      : undefined;
  const hostKey =
    isRecord(body) && isRecord(body.host_key) ? (body.host_key as HostKeyProblem) : undefined;
  return new ApiError(status, message, { conflict, retryAfter, space, hostKey });
}

/** The answer the page asked for before the app started, if there is a usable one. */
async function early(method: string, path: string): Promise<Response | null> {
  if (method !== 'GET') return null;
  try {
    const res = await takeEarly(path);
    return res?.ok ? res : null;
  } catch {
    return null; // the early request failed: ask again the normal way
  }
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  let res: Response;
  try {
    res =
      (await early(method, path)) ??
      (await fetch(`/api${path}`, {
        method,
        credentials: 'same-origin',
        headers: body === undefined ? undefined : { 'content-type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal,
      }));
  } catch {
    throw new ApiError(0, undefined);
  }
  return settle<T>(res, path);
}

/** Send bytes as they are (a `.torrent` file), not JSON. */
async function upload<T>(path: string, bytes: ArrayBuffer, contentType: string): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`/api${path}`, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': contentType },
      body: bytes,
    });
  } catch {
    throw new ApiError(0, undefined);
  }
  return settle<T>(res, path);
}

async function settle<T>(res: Response, path: string): Promise<T> {
  if (res.status === 204) {
    return undefined as T;
  }
  const data: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    if (res.status === 401 && path !== '/auth/login') {
      onUnauthorized?.();
    }
    throw parseError(res.status, data);
  }
  // The generated types are the contract; the server is trusted to honour it.
  return data as T;
}

export const api = {
  authStatus: () => request<AuthStatus>('GET', '/auth/status'),
  login: (password: string) => request<{ token: string }>('POST', '/auth/login', { password }),
  logout: () => request<void>('POST', '/auth/logout'),
  setup: (code: string, password: string) =>
    request<{ token: string }>('POST', '/auth/setup', { code, password }),
  changePassword: (current_password: string, new_password: string) =>
    request<void>('POST', '/auth/password', { current_password, new_password }),

  listDownloads: () => request<Download[]>('GET', '/downloads'),
  addDownload: (req: AddRequest) => request<Download>('POST', '/downloads', req),
  probeUrl: (url: string, credentials?: Credentials, signal?: AbortSignal) =>
    request<UrlInfo>('POST', '/probe', { url, credentials }, signal),
  getKnownHosts: () => request<KnownHost[]>('GET', '/known-hosts'),
  /** Accept the host key of an SSH server, as a refused request showed it. */
  trustHost: (host: TrustHost) => request<KnownHost>('POST', '/known-hosts', host),
  forgetHost: (id: number) => request<void>('DELETE', `/known-hosts/${id}`),
  pause: (id: number) => request<Download>('POST', `/downloads/${id}/pause`),
  resume: (id: number) => request<Download>('POST', `/downloads/${id}/resume`),
  cancel: (id: number) => request<Download>('POST', `/downloads/${id}/cancel`),
  /** Keep the file whatever the checksum says (while verifying, or after a failed verification). */
  keep: (id: number) => request<Download>('POST', `/downloads/${id}/keep`),
  remove: (id: number, deleteFile = false) =>
    request<void>('DELETE', `/downloads/${id}${deleteFile ? '?delete_file=true' : ''}`),
  setSpeedLimit: (id: number, speedLimit: number) =>
    request<Download>('PATCH', `/downloads/${id}`, { speed_limit: speedLimit }),
  setArchived: (id: number, archived: boolean) =>
    request<Download>('PATCH', `/downloads/${id}`, { archived }),
  startNow: (id: number) => request<Download>('PATCH', `/downloads/${id}`, { start_now: true }),
  setCategory: (id: number, category: Category) =>
    request<Download>('PATCH', `/downloads/${id}`, { category }),

  // Plugins (SPEC 6.15).
  listPlugins: () => request<PluginList>('GET', '/plugins'),
  rescanPlugins: () => request<PluginList>('POST', '/plugin-rescan'),
  /** The bytes of a plugin's ZIP file: unpacked and read, nothing runs until it is installed. */
  stagePlugin: (bytes: ArrayBuffer) =>
    upload<StagedPlugin>('/plugin-staging', bytes, 'application/zip'),
  stagePluginFromStore: (id: string) => request<StagedPlugin>('POST', '/plugin-staging/store', { id }),
  installStaged: (token: string) =>
    request<PluginView>('POST', `/plugin-staging/${encodeURIComponent(token)}/install`),
  cancelStaged: (token: string) =>
    request<void>('DELETE', `/plugin-staging/${encodeURIComponent(token)}`),
  setPluginEnabled: (id: string, enabled: boolean) =>
    request<PluginView>('PATCH', `/plugins/${encodeURIComponent(id)}`, { enabled }),
  approvePlugin: (id: string, hash: string) =>
    request<PluginView>('POST', `/plugins/${encodeURIComponent(id)}/approve`, { hash }),
  removePlugin: (id: string, keepData = false) =>
    request<void>('DELETE', `/plugins/${encodeURIComponent(id)}${keepData ? '?keep_data=true' : ''}`),
  /** `values` stores, `clear` empties; a key in neither stays as it is. */
  savePluginSettings: (id: string, values: Record<string, SettingValue>, clear: string[] = []) =>
    request<PluginView>('PUT', `/plugins/${encodeURIComponent(id)}/settings`, { values, clear }),
  runPluginAction: (id: string, action: string, input: Record<string, SettingValue> = {}) =>
    request<ActionResult>(
      'POST',
      `/plugins/${encodeURIComponent(id)}/actions/${encodeURIComponent(action)}`,
      { input },
    ),
  restartPlugin: (id: string) =>
    request<PluginView>('POST', `/plugins/${encodeURIComponent(id)}/restart`),
  /** One dependency (also an optional one), or all of them again when none is named. */
  fetchPluginDependencies: (id: string, dependency?: string) =>
    request<PluginView>(
      'POST',
      `/plugins/${encodeURIComponent(id)}/dependencies`,
      dependency === undefined ? {} : { dependency },
    ),
  pluginLogs: (id: string) =>
    request<{ lines: string[] }>('GET', `/plugins/${encodeURIComponent(id)}/logs`),
  pluginStore: () => request<StoreList>('GET', '/plugin-store'),
  refreshPluginStore: () => request<StoreList>('POST', '/plugin-store/refresh'),

  listTorrents: () => request<Torrent[]>('GET', '/torrents'),
  /** `disk`: where to save it (a name from `getDisks`); none = the default for torrents. */
  addTorrentMagnet: (magnet: string, disk?: string) =>
    request<Torrent>('POST', '/torrents', disk === undefined ? { magnet } : { magnet, disk }),
  /** The bytes of a `.torrent` file. */
  addTorrentFile: (bytes: ArrayBuffer, disk?: string) =>
    upload<Torrent>(
      disk === undefined ? '/torrents/file' : `/torrents/file?disk=${encodeURIComponent(disk)}`,
      bytes,
      'application/x-bittorrent',
    ),
  pauseTorrent: (id: number) => request<Torrent>('POST', `/torrents/${id}/pause`),
  resumeTorrent: (id: number) => request<Torrent>('POST', `/torrents/${id}/resume`),
  startTorrentNow: (id: number) => request<Torrent>('POST', `/torrents/${id}/start-now`),
  removeTorrent: (id: number, deleteFiles = false) =>
    request<void>('DELETE', `/torrents/${id}${deleteFiles ? '?delete_files=true' : ''}`),
  torrentPeers: (id: number, signal?: AbortSignal) =>
    request<TorrentPeers>('GET', `/torrents/${id}/peers`, undefined, signal),
  torrentFiles: (id: number, signal?: AbortSignal) =>
    request<TorrentFile[]>('GET', `/torrents/${id}/files`, undefined, signal),

  /** Space of a disk; without a name, of the one new downloads go to by default. */
  getDisk: (disk?: string) =>
    request<DiskSpace>('GET', disk === undefined ? '/disk' : `/disk?disk=${encodeURIComponent(disk)}`),
  /** The disks downloads can be saved on, with their space, and the default one. */
  getDisks: () => request<Disks>('GET', '/disks'),
  /** `offsetMinutes`: the caller's distance from UTC, east positive; it decides where a day begins. */
  getStats: (range: StatsRange, offsetMinutes: number) =>
    request<Stats>('GET', `/stats?range=${range}&offset=${offsetMinutes}`),
  getVersion: () => request<VersionInfo>('GET', '/version'),
  checkVersion: () => request<VersionInfo>('POST', '/version/check'),
  getSettings: () => request<Settings>('GET', '/settings'),
  putSettings: (settings: Settings) => request<Settings>('PUT', '/settings', settings),
};
