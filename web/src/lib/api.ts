// Browser API helpers for auth/session, snapshots, preferences, and SSE streams.

import type { Preferences, Series, SnapshotResponse } from './types';

interface SessionStateResponse {
  authenticated: boolean;
}

interface ErrorPayload {
  error?: string;
  retry_after_secs?: number;
}

export interface LoginResult {
  ok: boolean;
  error?: string;
  retryAfterSecs?: number;
}

export async function fetchSnapshot(series: Series): Promise<SnapshotResponse> {
  const response = await fetch(`/api/snapshot/${series}`);
  if (!response.ok) {
    throw new Error(`snapshot request failed (${response.status})`);
  }
  return response.json();
}

export async function fetchSessionState(): Promise<boolean> {
  const response = await fetch('/auth/session');
  if (!response.ok) {
    throw new Error(`session request failed (${response.status})`);
  }
  const payload = (await response.json()) as SessionStateResponse;
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

  const payload = (await safeReadJson(response)) as ErrorPayload | null;
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
    throw new Error(`preferences request failed (${response.status})`);
  }
  return response.json();
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
    throw new Error(`preferences update failed (${response.status})`);
  }

  return response.json();
}

export function openSeriesStream(series: Series, onSnapshot: (payload: SnapshotResponse) => void): EventSource {
  const eventSource = new EventSource(`/api/stream/${series}`);
  eventSource.addEventListener('snapshot', (event) => {
    try {
      const payload = JSON.parse((event as MessageEvent).data) as SnapshotResponse;
      onSnapshot(payload);
    } catch {
      // Ignore malformed events and keep stream alive.
    }
  });

  return eventSource;
}

async function safeReadJson(response: Response): Promise<unknown | null> {
  try {
    return await response.json();
  } catch {
    return null;
  }
}
