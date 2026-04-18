// Browser API helpers for auth/session, snapshots, preferences, and SSE streams.

import type { Preferences, Series, SnapshotResponse } from './types';

interface SessionStateResponse {
  authenticated: boolean;
}

interface ErrorPayload {
  error?: string;
  retry_after_secs?: number;
}

export interface DemoStateResponse {
  enabled: boolean;
}

export interface LoginResult {
  ok: boolean;
  error?: string;
  retryAfterSecs?: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isSeries(value: unknown): value is Series {
  return value === 'imsa' || value === 'nls' || value === 'f1' || value === 'wec';
}

function isSnapshotResponse(value: unknown): value is SnapshotResponse {
  if (!isRecord(value)) return false;
  return isSeries(value.series) && isRecord(value.snapshot);
}

function isSessionStateResponse(value: unknown): value is SessionStateResponse {
  return isRecord(value) && typeof value.authenticated === 'boolean';
}

function isDemoStateResponse(value: unknown): value is DemoStateResponse {
  return isRecord(value) && typeof value.enabled === 'boolean';
}

function isPreferences(value: unknown): value is Preferences {
  if (!isRecord(value)) return false;
  if (!Array.isArray(value.favourites) || !value.favourites.every((item) => typeof item === 'string')) {
    return false;
  }
  return isSeries(value.selected_series);
}

function readErrorPayload(value: unknown): ErrorPayload | null {
  if (!isRecord(value)) return null;
  const payload: ErrorPayload = {};
  if (typeof value.error === 'string') {
    payload.error = value.error;
  }
  if (typeof value.retry_after_secs === 'number') {
    payload.retry_after_secs = value.retry_after_secs;
  }
  return payload;
}

export async function fetchSnapshot(series: Series): Promise<SnapshotResponse> {
  const response = await fetch(`/api/snapshot/${series}`);
  if (!response.ok) {
    throw new Error(`snapshot request failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isSnapshotResponse(payload)) {
    throw new Error('snapshot response payload is invalid');
  }
  return payload;
}

export async function fetchSessionState(): Promise<boolean> {
  const response = await fetch('/auth/session');
  if (!response.ok) {
    throw new Error(`session request failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isSessionStateResponse(payload)) {
    throw new Error('session response payload is invalid');
  }
  return payload.authenticated;
}

export async function loginWithAccessCode(accessCode: string): Promise<LoginResult> {
  const response = await fetch('/auth/login', {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ access_code: accessCode })
  });

  if (response.ok) {
    return { ok: true };
  }

  const payload = readErrorPayload(await safeReadJson(response));
  return {
    ok: false,
    error: payload?.error ?? 'login failed',
    retryAfterSecs: payload?.retry_after_secs
  };
}

export async function logoutSession(): Promise<void> {
  await fetch('/auth/logout', { method: 'POST' });
}

export async function fetchPreferences(): Promise<Preferences> {
  const response = await fetch('/api/preferences');
  if (!response.ok) {
    throw new Error(`preferences request failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isPreferences(payload)) {
    throw new Error('preferences response payload is invalid');
  }
  return payload;
}

export async function updatePreferences(preferences: Preferences): Promise<Preferences> {
  const response = await fetch('/api/preferences', {
    method: 'PUT',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify(preferences)
  });

  if (!response.ok) {
    throw new Error(`preferences update failed (${String(response.status)})`);
  }

  const payload = await safeReadJson(response);
  if (!isPreferences(payload)) {
    throw new Error('preferences update payload is invalid');
  }
  return payload;
}

export async function fetchDemoState(): Promise<DemoStateResponse> {
  const response = await fetch('/api/demo');
  if (!response.ok) {
    throw new Error(`demo state request failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isDemoStateResponse(payload)) {
    throw new Error('demo state payload is invalid');
  }
  return payload;
}

export async function updateDemoState(enabled: boolean): Promise<DemoStateResponse> {
  const response = await fetch('/api/demo', {
    method: 'PUT',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ enabled })
  });
  if (!response.ok) {
    throw new Error(`demo state update failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isDemoStateResponse(payload)) {
    throw new Error('demo state update payload is invalid');
  }
  return payload;
}

export async function resetPreferences(): Promise<Preferences> {
  const response = await fetch('/api/preferences/reset', {
    method: 'POST'
  });
  if (!response.ok) {
    throw new Error(`preferences reset failed (${String(response.status)})`);
  }
  const payload = await safeReadJson(response);
  if (!isPreferences(payload)) {
    throw new Error('preferences reset payload is invalid');
  }
  return payload;
}

export function openSeriesStream(series: Series, onSnapshot: (payload: SnapshotResponse) => void): EventSource {
  const eventSource = new EventSource(`/api/stream/${series}`);
  eventSource.addEventListener('snapshot', (event) => {
    try {
      if (!(event instanceof MessageEvent) || typeof event.data !== 'string') {
        return;
      }
      const payload = JSON.parse(event.data) as unknown;
      if (isSnapshotResponse(payload)) {
        onSnapshot(payload);
      }
    } catch {
      // Ignore malformed events and keep stream alive.
    }
  });

  return eventSource;
}

async function safeReadJson(response: Response): Promise<unknown> {
  try {
    return (await response.json()) as unknown;
  } catch {
    return null;
  }
}
