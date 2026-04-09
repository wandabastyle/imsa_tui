import type { Preferences, Series, SnapshotResponse } from './types';

export async function fetchSnapshot(series: Series): Promise<SnapshotResponse> {
  const response = await fetch(`/api/snapshot/${series}`);
  if (!response.ok) {
    throw new Error(`snapshot request failed (${response.status})`);
  }
  return response.json();
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
