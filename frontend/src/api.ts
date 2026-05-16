import {
  ScanResponse,
  SettingsResponse,
  SessionMutationResponse,
  SessionsResponse,
  ActivitySummaryResponse,
  CollaborationStateResponse,
  CollaborationSummary,
  CreateSubscriptionResponse,
  IncrementalSummaryResponse,
  PairPeerResponse,
  PeerProject,
} from './types';

const API_BASE_URL = (import.meta.env.VITE_API_BASE_URL || '').replace(/\/$/, '');
let tauriApiBaseUrlPromise: Promise<string> | null = null;

interface ApiErrorBody {
  error?: {
    message?: string;
  };
}

async function requestJson<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetchWithStartupRetry(await apiUrl(path), {
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
    ...options,
  });

  if (!response.ok) {
    let message = `Request failed with ${response.status}`;
    try {
      const body = (await response.json()) as ApiErrorBody;
      message = body.error?.message || message;
    } catch {
      // Keep the status-based message when the server returns non-JSON.
    }
    throw new Error(message);
  }

  return response.json() as Promise<T>;
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function apiUrl(path: string) {
  if (API_BASE_URL) {
    return `${API_BASE_URL}${path}`;
  }

  if (isTauriRuntime()) {
    return `${await getTauriApiBaseUrl()}${path}`;
  }

  return path;
}

async function getTauriApiBaseUrl() {
  if (!tauriApiBaseUrlPromise) {
    tauriApiBaseUrlPromise = invokeCommand<string>('get_collaboration_api_base_url')
      .then((value) => value.replace(/\/$/, ''))
      .then((value) => {
        if (!value) {
          throw new Error('Desktop collaboration API URL is unavailable');
        }
        return value;
      });
  }

  return tauriApiBaseUrlPromise;
}

async function fetchWithStartupRetry(url: string, init: RequestInit) {
  const maxAttempts = isTauriRuntime() ? 3 : 1;
  let lastError: unknown;

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    try {
      return await fetch(url, init);
    } catch (error) {
      lastError = error;
      if (attempt === maxAttempts) break;
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
  }

  throw lastError;
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export function fetchSessions() {
  if (isTauriRuntime()) {
    return invokeCommand<SessionsResponse>('get_sessions');
  }

  return requestJson<SessionsResponse>('/api/sessions');
}

export function scanSessions(path: string) {
  if (isTauriRuntime()) {
    return invokeCommand<ScanResponse>('scan_sessions', { path });
  }

  return requestJson<ScanResponse>('/api/sessions/scan', {
    method: 'POST',
    body: JSON.stringify({ path }),
  });
}

export function updateSettings(staleAfterDays: number) {
  if (isTauriRuntime()) {
    return invokeCommand<SettingsResponse>('update_settings', { staleAfterDays });
  }

  return requestJson<SettingsResponse>('/api/settings', {
    method: 'PATCH',
    body: JSON.stringify({ staleAfterDays }),
  });
}

export function generateActivitySummary(days = 7, language = 'zh') {
  if (isTauriRuntime()) {
    return invokeCommand<ActivitySummaryResponse>('generate_activity_summary', { days, language });
  }

  return requestJson<ActivitySummaryResponse>('/api/summaries/activity', {
    method: 'POST',
    body: JSON.stringify({ days, language }),
  });
}

export function fetchCollaborationState() {
  return requestJson<CollaborationStateResponse>('/api/collaboration');
}

export function updateSharePolicy(
  projectId: string,
  payload: {
    projectPath?: string | null;
    enabled?: boolean;
    sharedLabels?: string[];
    blockedLabels?: string[];
    maxExcerptChars?: number;
    maxDeltaChars?: number;
  }
) {
  return requestJson<CollaborationStateResponse>(
    `/api/collaboration/share-policies/${encodeURIComponent(projectId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }
  );
}

export function generateCollaborationBaseline(payload: {
  peerBaseUrl: string;
  peerAccessToken?: string;
  projectId: string;
  peerId?: string;
  days?: number;
  language?: string;
}) {
  return requestJson<CollaborationSummary>('/api/collaboration/summaries/baseline', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function pairPeer(payload: {
  peerBaseUrl: string;
  peerAccessToken?: string;
  displayName?: string;
}) {
  return requestJson<PairPeerResponse>('/api/collaboration/peers/pair', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function fetchCollaborationPeerProjects(peerId: string, peerAccessToken?: string) {
  return requestJson<PeerProject[]>(
    `/api/collaboration/peers/${encodeURIComponent(peerId)}/projects`,
    {
      method: 'POST',
      body: JSON.stringify({ peerAccessToken }),
    }
  );
}

export function createCollaborationSubscription(payload: {
  peerId?: string;
  peerBaseUrl?: string;
  peerAccessToken?: string;
  projectId: string;
  days?: number;
  language?: string;
  topics?: string[];
}) {
  return requestJson<CreateSubscriptionResponse>('/api/collaboration/subscriptions', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function generateCollaborationIncremental(payload: {
  subscriptionId: string;
  peerAccessToken?: string;
  since?: string;
  language?: string;
}) {
  return requestJson<IncrementalSummaryResponse>('/api/collaboration/summaries/incremental', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function updateSessionLabels(id: string, labels: string[]) {
  if (isTauriRuntime()) {
    return invokeCommand<SessionMutationResponse>('update_session_labels', { id, labels });
  }

  return requestJson<SessionMutationResponse>(`/api/sessions/${id}/labels`, {
    method: 'PATCH',
    body: JSON.stringify({ labels }),
  });
}

export function updateSessionNotes(id: string, notes: string) {
  if (isTauriRuntime()) {
    return invokeCommand<SessionMutationResponse>('update_session_notes', { id, notes });
  }

  return requestJson<SessionMutationResponse>(`/api/sessions/${id}/notes`, {
    method: 'PATCH',
    body: JSON.stringify({ notes }),
  });
}

export function archiveDeleteSession(id: string) {
  if (isTauriRuntime()) {
    return invokeCommand<SessionMutationResponse>('archive_delete_session', { id });
  }

  return requestJson<SessionMutationResponse>(`/api/sessions/${id}/archive-delete`, {
    method: 'POST',
  });
}

export function restoreSession(id: string) {
  if (isTauriRuntime()) {
    return invokeCommand<SessionMutationResponse>('restore_session', { id });
  }

  return requestJson<SessionMutationResponse>(`/api/sessions/${id}/restore`, {
    method: 'POST',
  });
}
