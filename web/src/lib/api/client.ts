// Thin typed wrapper over fetch for the REST API. Types come from the generated schema.
import { takeEarly } from './early';
import type {
  AddRequest,
  AuthStatus,
  Category,
  Conflict,
  DiskSpace,
  Download,
  Settings,
  Stats,
  StatsRange,
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

  constructor(
    status: number,
    serverMessage: string | undefined,
    extra: {
      conflict?: Conflict;
      retryAfter?: number;
      space?: SpaceInfo;
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
  return new ApiError(status, message, { conflict, retryAfter, space });
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
  probeUrl: (url: string, signal?: AbortSignal) =>
    request<UrlInfo>('POST', '/probe', { url }, signal),
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

  getDisk: () => request<DiskSpace>('GET', '/disk'),
  /** `offsetMinutes`: the caller's distance from UTC, east positive; it decides where a day begins. */
  getStats: (range: StatsRange, offsetMinutes: number) =>
    request<Stats>('GET', `/stats?range=${range}&offset=${offsetMinutes}`),
  getVersion: () => request<VersionInfo>('GET', '/version'),
  checkVersion: () => request<VersionInfo>('POST', '/version/check'),
  getSettings: () => request<Settings>('GET', '/settings'),
  putSettings: (settings: Settings) => request<Settings>('PUT', '/settings', settings),
};
