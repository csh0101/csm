import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { MainList } from './components/MainList';
import { PreviewPanel } from './components/PreviewPanel';
import { CollaborationPanel } from './components/CollaborationPanel';
import {
  CollaborationStateResponse,
  CollaborationSummary,
  FilterType,
  PeerPresence,
  PeerProject,
  Session,
  SessionsResponse,
  SortDirection,
  SortField,
  ActivitySummaryResponse,
  labelFromFilter,
  projectPathFromFilter,
} from './types';
import { formatDistanceToNow, subDays } from 'date-fns';
import { useI18n } from './i18n';
import { Bell, CheckCircle, X } from 'lucide-react';
import { cn } from './lib/utils';
import {
  archiveDeleteSession,
  createCollaborationSubscription,
  deleteTrustedPeer,
  fetchSessionContent,
  fetchCollaborationPeerProjects,
  fetchSessions,
  fetchCollaborationState,
  generateCollaborationIncremental,
  generateActivitySummary,
  pairPeer,
  restoreSession,
  scanSessions,
  updateLocalCollaborationConfig,
  updateSubscriptionSchedule,
  updateSharePolicy,
  updateSessionLabels,
  updateSessionNotes,
  updateSessionContent,
  updateSettings,
  updateTrustedPeerConnection,
} from './api';

const DEFAULT_STALE_AFTER_DAYS = 15;

type NotificationTaskResult = {
  time: string;
  status: 'success' | 'failed';
  result: string;
};

type AppNotification = {
  id: string;
  title: string;
  message: string;
  time: string;
  sortTime: number;
  unread: boolean;
  taskResult: NotificationTaskResult;
};

function normalizePeerBaseUrl(value: string) {
  const trimmed = value.trim().replace(/\/+$/, '');
  if (!trimmed) return '';
  return trimmed.includes('://') ? trimmed : `http://${trimmed}`;
}

function errorMessageFrom(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

function formatCollaborationError(
  error: unknown,
  fallback: string,
  t: (key: string, params?: Record<string, string | number>) => string
) {
  const message = errorMessageFrom(error, fallback);
  const normalized = message.toLowerCase();

  if (
    normalized.includes('failed to reach peer') ||
    normalized.includes('error sending request') ||
    normalized.includes('failed to fetch') ||
    normalized.includes("couldn't connect") ||
    normalized.includes('connection refused') ||
    normalized.includes('timed out')
  ) {
    return [
      t('error_collaboration_peer_unreachable'),
      t('error_collaboration_peer_unreachable_hint'),
      t('error_details', { message }),
    ].join('\n');
  }

  if (
    normalized.includes('http 401') ||
    normalized.includes('http 403') ||
    normalized.includes('peer rejected read api')
  ) {
    return [
      t('error_collaboration_peer_rejected'),
      t('error_collaboration_peer_rejected_hint'),
      t('error_details', { message }),
    ].join('\n');
  }

  if (normalized.includes('invalid json')) {
    return [
      t('error_collaboration_peer_invalid_response'),
      t('error_details', { message }),
    ].join('\n');
  }

  return message;
}

export default function App() {
  const { t, lang } = useI18n();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeView, setActiveView] = useState<'sessions' | 'collaboration'>('sessions');
  const [currentFilter, setCurrentFilter] = useState<FilterType>('all');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedLabelFilters, setSelectedLabelFilters] = useState<string[]>([]);
  const [workspacePath, setWorkspacePath] = useState('~/.codex/sessions');
  const [staleAfterDays, setStaleAfterDays] = useState(DEFAULT_STALE_AFTER_DAYS);
  const [staleAfterDaysDraft, setStaleAfterDaysDraft] = useState(String(DEFAULT_STALE_AFTER_DAYS));
  const [sortField, setSortField] = useState<SortField>('lastModified');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const [isLoading, setIsLoading] = useState(false);
  const [isMutating, setIsMutating] = useState(false);
  const [isSettingsSaving, setIsSettingsSaving] = useState(false);
  const [isGeneratingSummary, setIsGeneratingSummary] = useState(false);
  const [summaryDays, setSummaryDays] = useState(7);
  const [activitySummary, setActivitySummary] = useState<ActivitySummaryResponse | null>(null);
  const [collaborationState, setCollaborationState] = useState<CollaborationStateResponse | null>(null);
  const [peerBaseUrl, setPeerBaseUrl] = useState('');
  const [peerAccessToken, setPeerAccessToken] = useState('');
  const [selectedPeerId, setSelectedPeerId] = useState('');
  const selectedPeerIdRef = useRef('');
  const hasInitializedCollaborationSelectionRef = useRef(false);
  const [peerProjects, setPeerProjects] = useState<PeerProject[]>([]);
  const [collaborationProjectId, setCollaborationProjectId] = useState('');
  const [collaborationSummaryDays, setCollaborationSummaryDays] = useState(7);
  const [isCollaborationLoading, setIsCollaborationLoading] = useState(false);
  const [isPairingPeer, setIsPairingPeer] = useState(false);
  const [isLoadingPeerProjects, setIsLoadingPeerProjects] = useState(false);
  const [isSavingSharePolicy, setIsSavingSharePolicy] = useState(false);
  const [isUpdatingPeerToken, setIsUpdatingPeerToken] = useState(false);
  const [isDeletingPeer, setIsDeletingPeer] = useState(false);
  const [isRefreshingLocalToken, setIsRefreshingLocalToken] = useState(false);
  const [generatingCollaborationProjectIds, setGeneratingCollaborationProjectIds] = useState<string[]>([]);
  const generatingCollaborationProjectIdsRef = useRef<Set<string>>(new Set());
  const [isRefreshingCollaboration, setIsRefreshingCollaboration] = useState(false);
  const [latestCollaborationSummary, setLatestCollaborationSummary] = useState<CollaborationSummary | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [noticeMessage, setNoticeMessage] = useState<string | null>(null);
  const [collaborationErrorMessage, setCollaborationErrorMessage] = useState<string | null>(null);
  const [collaborationNoticeMessage, setCollaborationNoticeMessage] = useState<string | null>(null);
  const appStartedAtRef = useRef(Date.now());
  const [showNotifications, setShowNotifications] = useState(false);
  const [readNotificationIds, setReadNotificationIds] = useState<string[]>([]);
  const [selectedTaskResult, setSelectedTaskResult] = useState<NotificationTaskResult | null>(null);

  useEffect(() => {
    selectedPeerIdRef.current = selectedPeerId;
  }, [selectedPeerId]);

  const applySessionsResponse = (response: SessionsResponse) => {
    setSessions(response.sessions);
    if (response.workspacePath) {
      setWorkspacePath(response.workspacePath);
    }
    if (Number.isFinite(response.staleAfterDays) && response.staleAfterDays > 0) {
      setStaleAfterDays(response.staleAfterDays);
      setStaleAfterDaysDraft(String(response.staleAfterDays));
    }
  };

  const loadProjectsForPeer = useCallback(async (
    peerId: string,
    peers: CollaborationStateResponse['store']['trustedPeers'],
    reportErrors = true,
    showLoading = true
  ) => {
    const peer = peers.find((item) => item.peerId === peerId);
    if (!peer?.baseUrl) return;

    setPeerBaseUrl(peer.baseUrl);
    if (showLoading) {
      setIsLoadingPeerProjects(true);
    }
    if (reportErrors) {
      setCollaborationErrorMessage(null);
      setCollaborationNoticeMessage(null);
    }
    try {
      const projects = await fetchCollaborationPeerProjects(peerId);
      setPeerProjects(projects);
      setCollaborationProjectId(projects[0]?.projectId || '');
    } catch (error) {
      if (reportErrors) {
        setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_projects_failed'), t));
      }
    } finally {
      if (showLoading) {
        setIsLoadingPeerProjects(false);
      }
    }
  }, [t]);

  useEffect(() => {
    let isMounted = true;

    fetchSessions()
      .then((response) => {
        if (!isMounted) return;
        applySessionsResponse(response);
        setNoticeMessage(null);
      })
      .catch((error) => {
        if (!isMounted) return;
        setErrorMessage(error instanceof Error ? error.message : 'Failed to load sessions');
      });

    return () => {
      isMounted = false;
    };
  }, []);

  const loadCollaborationState = useCallback(async (showLoading = true) => {
    if (showLoading) {
      setIsCollaborationLoading(true);
    }
    try {
      const response = await fetchCollaborationState();
      const currentPeerId = selectedPeerIdRef.current;
      const currentPeerStillExists = response.store.trustedPeers.some(
        (peer) => peer.peerId === currentPeerId
      );
      const nextSelectedPeerId =
        currentPeerId && currentPeerStillExists
          ? currentPeerId
          : !hasInitializedCollaborationSelectionRef.current
            ? response.store.trustedPeers[0]?.peerId || ''
            : '';
      hasInitializedCollaborationSelectionRef.current = true;
      setCollaborationState(response);
      setCollaborationProjectId((current) => current || response.projects[0]?.projectId || '');
      selectedPeerIdRef.current = nextSelectedPeerId;
      setSelectedPeerId(nextSelectedPeerId);
      if (nextSelectedPeerId) {
        void loadProjectsForPeer(nextSelectedPeerId, response.store.trustedPeers, false, false);
      }
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_load_failed'), t));
    } finally {
      if (showLoading) {
        setIsCollaborationLoading(false);
      }
    }
  }, [loadProjectsForPeer, t]);

  useEffect(() => {
    void loadCollaborationState();
  }, [loadCollaborationState]);

  useEffect(() => {
    if (activeView !== 'collaboration') return;

    void loadCollaborationState();
    const refreshId = window.setInterval(() => {
      void loadCollaborationState(false);
    }, 3000);

    return () => window.clearInterval(refreshId);
  }, [activeView, loadCollaborationState]);

  // Derived state
  const { filteredSessions, counts } = useMemo(() => {
    let filtered = sessions;
    const now = new Date();
    const sevenDaysAgo = subDays(now, 7);
    const staleCutoff = subDays(now, staleAfterDays);
    const labelFilter = labelFromFilter(currentFilter);
    const projectPathFilter = projectPathFromFilter(currentFilter);
    
    // Always separate deleted from non-deleted for standard filters
    const active = sessions.filter(s => s.status !== 'deleted');
    const deleted = sessions.filter(s => s.status === 'deleted');
    
    if (currentFilter === 'all') {
      filtered = active;
    } else if (currentFilter === 'deleted') {
      filtered = deleted;
    } else if (currentFilter === 'recent') {
      filtered = active.filter(s => new Date(s.lastModified) >= sevenDaysAgo);
    } else if (currentFilter === 'stale') {
      filtered = active.filter(s => s.status === 'stale' || new Date(s.lastModified) < staleCutoff);
    } else if (currentFilter === 'unlabeled') {
      filtered = active.filter(s => s.labels.length === 0);
    } else if (labelFilter) {
      filtered = active.filter(s => s.labels.includes(labelFilter));
    } else if (projectPathFilter) {
      filtered = active.filter(s => s.projectPath === projectPathFilter);
    }

    if (selectedLabelFilters.length > 0) {
      filtered = filtered.filter(s =>
        selectedLabelFilters.every(label => s.labels.includes(label))
      );
    }

    const searchTerms = searchQuery.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (searchTerms.length > 0) {
      filtered = filtered.filter(s => {
        const searchableText = [
          s.name,
          s.excerpt,
          s.path,
          s.projectPath || '',
          ...s.labels
        ].join('\n').toLowerCase();

        return searchTerms.every(term => searchableText.includes(term));
      });
    }

    const sorted = [...filtered].sort((a, b) => {
      const result = sortField === 'size'
        ? a.size - b.size
        : new Date(a.lastModified).getTime() - new Date(b.lastModified).getTime();

      if (result !== 0) {
        return sortDirection === 'asc' ? result : -result;
      }

      return a.name.localeCompare(b.name);
    });

    return {
      filteredSessions: sorted,
      counts: {
        all: active.length,
        recent: active.filter(s => new Date(s.lastModified) >= sevenDaysAgo).length,
        stale: active.filter(s => s.status === 'stale' || new Date(s.lastModified) < staleCutoff).length,
        unlabeled: active.filter(s => s.labels.length === 0).length,
        deleted: deleted.length,
      }
    };
  }, [sessions, currentFilter, selectedLabelFilters, searchQuery, staleAfterDays, sortField, sortDirection]);

  const focusedSession = useMemo(() => {
    return sessions.find(s => s.id === focusedId) || null;
  }, [sessions, focusedId]);

  // Handlers
  const handleToggleSelect = (id: string) => {
    const newTarget = new Set(selectedIds);
    if (newTarget.has(id)) {
      newTarget.delete(id);
    } else {
      newTarget.add(id);
    }
    setSelectedIds(newTarget);
  };

  const handleToggleLabelFilter = (label: string) => {
    setSelectedLabelFilters(prev =>
      prev.includes(label)
        ? prev.filter(existing => existing !== label)
        : [...prev, label]
    );
    setSelectedIds(new Set());
    setFocusedId(null);
  };

  const handleSortChange = (field: SortField) => {
    if (field === sortField) {
      setSortDirection(prev => prev === 'asc' ? 'desc' : 'asc');
      return;
    }

    setSortField(field);
    setSortDirection('desc');
  };

  const replaceSession = (updatedSession: Session) => {
    setSessions(prev => prev.map(s => s.id === updatedSession.id ? updatedSession : s));
  };

  const handleScan = async () => {
    setIsLoading(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await scanSessions(workspacePath);
      applySessionsResponse(response);
      if (response.skippedFiles > 0) {
        setNoticeMessage(t('scan_skipped_files', { count: response.skippedFiles }));
      }
      setSelectedIds(new Set());
      setFocusedId(response.sessions[0]?.id || null);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_scan_failed'));
    } finally {
      setIsLoading(false);
    }
  };

  const handleSaveStaleAfterDays = async () => {
    const parsed = Number.parseInt(staleAfterDaysDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 3650) {
      setErrorMessage(t('error_invalid_stale_threshold'));
      return;
    }

    setIsSettingsSaving(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await updateSettings(parsed);
      applySessionsResponse(response);
      setNoticeMessage(t('settings_saved'));
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_save_settings_failed'));
    } finally {
      setIsSettingsSaving(false);
    }
  };

  const handleGenerateActivitySummary = async () => {
    setIsGeneratingSummary(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await generateActivitySummary(summaryDays, lang);
      setActivitySummary(response);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_summary_failed'));
    } finally {
      setIsGeneratingSummary(false);
    }
  };

  const handleToggleSharePolicy = async (projectId: string, projectPath: string | null, enabled: boolean) => {
    setIsSavingSharePolicy(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await updateSharePolicy(projectId, {
        projectPath,
        enabled,
        sharedLabels: ['share', 'team', 'review', 'collab'],
        blockedLabels: ['private', 'secret'],
      });
      setCollaborationState(response);
      setCollaborationNoticeMessage(t('collab_share_policy_saved'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_save_failed'), t));
    } finally {
      setIsSavingSharePolicy(false);
    }
  };

  const handleUpdateLocalDisplayName = async (displayName: string) => {
    const trimmed = displayName.trim();
    if (!trimmed) return;

    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await updateLocalCollaborationConfig({ displayName: trimmed });
      setCollaborationState(response);
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_save_failed'), t));
    }
  };

  const handleRefreshLocalPeerToken = async () => {
    setIsRefreshingLocalToken(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await updateLocalCollaborationConfig({ refreshPeerToken: true });
      setCollaborationState(response);
      setCollaborationNoticeMessage(t('collab_local_token_refreshed'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_save_failed'), t));
    } finally {
      setIsRefreshingLocalToken(false);
    }
  };

  const handleUpdatePeerConnection = async (peerId: string, baseUrl: string, token?: string) => {
    const normalizedPeerBaseUrl = normalizePeerBaseUrl(baseUrl);
    if (!peerId || !normalizedPeerBaseUrl) return;
    const trimmedToken = token?.trim();

    setIsUpdatingPeerToken(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await updateTrustedPeerConnection(peerId, {
        peerBaseUrl: normalizedPeerBaseUrl,
        peerAccessToken: trimmedToken || undefined,
      });
      setCollaborationState(response);
      setPeerBaseUrl(normalizedPeerBaseUrl);
      setPeerAccessToken('');
      setCollaborationNoticeMessage(t('collab_peer_connection_saved'));
      if (peerId === selectedPeerId) {
        try {
          const projects = await fetchCollaborationPeerProjects(peerId);
          setPeerProjects(projects);
          setCollaborationProjectId(projects[0]?.projectId || '');
        } catch (error) {
          setPeerProjects([]);
          setCollaborationProjectId('');
          setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_projects_failed'), t));
        }
      }
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_save_failed'), t));
    } finally {
      setIsUpdatingPeerToken(false);
    }
  };

  const handleDeleteTrustedPeer = async (peerId: string) => {
    if (!peerId || isDeletingPeer) return;

    setIsDeletingPeer(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await deleteTrustedPeer(peerId);
      setCollaborationState(response);
      if (selectedPeerIdRef.current === peerId) {
        selectedPeerIdRef.current = '';
        setSelectedPeerId('');
        setPeerBaseUrl('');
        setPeerAccessToken('');
        setPeerProjects([]);
        setCollaborationProjectId('');
        setLatestCollaborationSummary(null);
      }
      setCollaborationNoticeMessage(t('collab_peer_deleted'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_delete_peer_failed'), t));
    } finally {
      setIsDeletingPeer(false);
    }
  };

  const handlePairPeer = async () => {
    if (!peerBaseUrl.trim()) return;

    setIsPairingPeer(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const normalizedPeerBaseUrl = normalizePeerBaseUrl(peerBaseUrl);
      setPeerBaseUrl(normalizedPeerBaseUrl);
      const response = await pairPeer({
        peerBaseUrl: normalizedPeerBaseUrl,
        peerAccessToken: peerAccessToken.trim() || undefined,
      });
      setCollaborationState(response.state);
      setPeerProjects(response.peerProjects);
      selectedPeerIdRef.current = response.peer.peerId;
      setSelectedPeerId(response.peer.peerId);
      setPeerAccessToken('');
      setCollaborationNoticeMessage(t('collab_peer_paired'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_pair_failed'), t));
    } finally {
      setIsPairingPeer(false);
    }
  };

  const handleUseDiscoveredPeer = (peer: PeerPresence) => {
    setPeerBaseUrl(peer.baseUrl);
    selectedPeerIdRef.current = '';
    setSelectedPeerId('');
    setPeerProjects([]);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
  };

  const handleSelectPeer = async (peerId: string) => {
    selectedPeerIdRef.current = peerId;
    setSelectedPeerId(peerId);
    setCollaborationProjectId('');
    setPeerProjects([]);
    await loadProjectsForPeer(peerId, collaborationState?.store.trustedPeers ?? []);
  };

  const handleCreateCollaborationSubscription = async (
    projectId?: string,
    analysisCycle: '10m' | '1h' | 'manual' = '1h'
  ) => {
    const targetProjectId = projectId || collaborationProjectId;
    if (!targetProjectId || (!selectedPeerId && !peerBaseUrl.trim())) return;
    if (generatingCollaborationProjectIdsRef.current.has(targetProjectId)) return;

    generatingCollaborationProjectIdsRef.current.add(targetProjectId);
    setCollaborationProjectId(targetProjectId);
    setGeneratingCollaborationProjectIds((current) =>
      current.includes(targetProjectId) ? current : [...current, targetProjectId]
    );
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const normalizedPeerBaseUrl = normalizePeerBaseUrl(peerBaseUrl);
      if (!selectedPeerId) {
        setPeerBaseUrl(normalizedPeerBaseUrl);
      }
      const response = await createCollaborationSubscription({
        peerId: selectedPeerId || undefined,
        peerBaseUrl: selectedPeerId ? undefined : normalizedPeerBaseUrl,
        peerAccessToken: peerAccessToken.trim() || undefined,
        projectId: targetProjectId,
        days: collaborationSummaryDays,
        language: lang,
        analysisCycle,
      });
      setCollaborationState(response.state);
      setLatestCollaborationSummary(response.summary);
      selectedPeerIdRef.current = response.subscription.peerId;
      setSelectedPeerId(response.subscription.peerId);
      setCollaborationProjectId(response.subscription.projectId);
      setCollaborationNoticeMessage(t('collab_subscription_created'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_summary_failed'), t));
    } finally {
      generatingCollaborationProjectIdsRef.current.delete(targetProjectId);
      setGeneratingCollaborationProjectIds((current) => current.filter((id) => id !== targetProjectId));
    }
  };

  const handleGenerateCollaborationIncremental = async () => {
    const subscription = collaborationState?.store.subscriptions.find(
      (item) => item.peerId === selectedPeerId && item.projectId === collaborationProjectId && item.status === 'active'
    );
    if (!subscription) return;

    setIsRefreshingCollaboration(true);
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await generateCollaborationIncremental({
        subscriptionId: subscription.subscriptionId,
        peerAccessToken: peerAccessToken.trim() || undefined,
        language: lang,
      });
      setCollaborationState(response.state);
      setLatestCollaborationSummary(response.summary);
      setCollaborationNoticeMessage(t('collab_incremental_created'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_incremental_failed'), t));
    } finally {
      setIsRefreshingCollaboration(false);
    }
  };

  const handleUpdateSubscriptionSchedule = async (
    subscriptionId: string,
    analysisCycle: '10m' | '1h' | 'manual'
  ) => {
    setCollaborationErrorMessage(null);
    setCollaborationNoticeMessage(null);
    try {
      const response = await updateSubscriptionSchedule(subscriptionId, analysisCycle);
      setCollaborationState(response);
      setCollaborationNoticeMessage(t('collab_schedule_saved'));
    } catch (error) {
      setCollaborationErrorMessage(formatCollaborationError(error, t('error_collaboration_save_failed'), t));
    }
  };

  const handleDelete = async (id: string) => {
    setIsMutating(true);
    setErrorMessage(null);
    try {
      const response = await archiveDeleteSession(id);
      replaceSession(response.session);
      if (focusedId === id) setFocusedId(null);
      const newSelected = new Set(selectedIds);
      newSelected.delete(id);
      setSelectedIds(newSelected);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_delete_failed'));
    } finally {
      setIsMutating(false);
    }
  };

  const handleBulkArchiveDelete = async () => {
    const idsToArchive = Array.from(selectedIds);
    if (idsToArchive.length === 0 || currentFilter === 'deleted') return;

    setIsMutating(true);
    setErrorMessage(null);

    const archivedIds: string[] = [];
    try {
      for (const id of idsToArchive) {
        const sessionName = sessions.find(s => s.id === id)?.name || id;
        try {
          const response = await archiveDeleteSession(id);
          archivedIds.push(id);
          replaceSession(response.session);
        } catch (error) {
          const message = error instanceof Error ? error.message : t('error_delete_failed');
          if (focusedId && archivedIds.includes(focusedId)) {
            setFocusedId(null);
          }
          setSelectedIds(prev => {
            const nextSelected = new Set(prev);
            archivedIds.forEach(archivedId => nextSelected.delete(archivedId));
            return nextSelected;
          });
          setErrorMessage(t('error_bulk_archive_failed', { name: sessionName, message }));
          return;
        }
      }

      if (focusedId && archivedIds.includes(focusedId)) {
        setFocusedId(null);
      }
      setSelectedIds(new Set());
    } finally {
      setIsMutating(false);
    }
  };

  const handleRestore = async (id: string) => {
    setIsMutating(true);
    setErrorMessage(null);
    try {
      const response = await restoreSession(id);
      replaceSession(response.session);
      if (focusedId === id) setFocusedId(null);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_restore_failed'));
    } finally {
      setIsMutating(false);
    }
  };

  const handleUpdateLabels = async (id: string, labels: string[]) => {
    setIsMutating(true);
    setErrorMessage(null);
    try {
      const response = await updateSessionLabels(id, labels);
      replaceSession(response.session);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_save_failed'));
    } finally {
      setIsMutating(false);
    }
  };

  const handleUpdateNotes = async (id: string, notes: string) => {
    setIsMutating(true);
    setErrorMessage(null);
    try {
      const response = await updateSessionNotes(id, notes);
      replaceSession(response.session);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_save_failed'));
    } finally {
      setIsMutating(false);
    }
  };

  const handleUpdateContent = async (id: string, content: string) => {
    setIsMutating(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await updateSessionContent(id, content);
      replaceSession(response.session);
      setNoticeMessage(t('content_editor_saved'));
      return response;
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_save_session_content_failed'));
      throw error;
    } finally {
      setIsMutating(false);
    }
  };

  const selectedLabel = labelFromFilter(currentFilter);
  const selectedProjectPath = projectPathFromFilter(currentFilter);
  const projectPathTitle = selectedProjectPath
    ? selectedProjectPath.split('/').filter(Boolean).slice(-2).join('/') || selectedProjectPath
    : '';
  const explorerScopeKey = `${currentFilter}\u001f${selectedLabelFilters.join('\u001f')}`;

  // Human readable title for the main list
  const filterTitle = selectedLabel
    ? t('filter_label', { label: selectedLabel })
    : selectedProjectPath
      ? t('filter_project_path', { path: projectPathTitle })
      : ({
        all: t('filter_all'),
        recent: t('filter_recent'),
        stale: t('filter_stale'),
        deleted: t('filter_deleted'),
        unlabeled: t('filter_unlabeled')
      }[currentFilter] || t('filter_all'));

  const notifications = useMemo<AppNotification[]>(() => {
    const projectLabelById = new Map(
      (collaborationState?.projects ?? []).map((project) => [project.projectId, project.pathLabel])
    );
    const readIds = new Set(readNotificationIds);
    const items: AppNotification[] = (collaborationState?.store.summaries ?? []).map((summary) => {
      const generatedAt = new Date(summary.generatedAt);
      const generatedTime = generatedAt.getTime();
      const projectLabel = projectLabelById.get(summary.projectId) ?? summary.projectId;
      const isBaseline = summary.engine === 'codex-exec';
      const id = `summary:${summary.summaryId}`;

      return {
        id,
        title: isBaseline ? t('notification_baseline_completed') : t('notification_incremental_completed'),
        message: t('notification_analysis_message', { project: projectLabel }),
        time: Number.isFinite(generatedTime)
          ? formatDistanceToNow(generatedAt, { addSuffix: true })
          : '',
        sortTime: Number.isFinite(generatedTime) ? generatedTime : 0,
        unread: generatedTime > appStartedAtRef.current && !readIds.has(id),
        taskResult: {
          time: Number.isFinite(generatedTime) ? generatedAt.toLocaleString() : summary.generatedAt,
          status: 'success',
          result: summary.markdown,
        },
      };
    });

    if (activitySummary) {
      const generatedAt = new Date(activitySummary.generatedAt);
      const generatedTime = generatedAt.getTime();
      const id = `activity:${activitySummary.generatedAt}`;
      items.push({
        id,
        title: t('activity_summary_title'),
        message: t('notification_activity_message', { count: activitySummary.includedSessionCount }),
        time: Number.isFinite(generatedTime)
          ? formatDistanceToNow(generatedAt, { addSuffix: true })
          : '',
        sortTime: Number.isFinite(generatedTime) ? generatedTime : 0,
        unread: generatedTime > appStartedAtRef.current && !readIds.has(id),
        taskResult: {
          time: Number.isFinite(generatedTime) ? generatedAt.toLocaleString() : activitySummary.generatedAt,
          status: 'success',
          result: activitySummary.summary,
        },
      });
    }

    return items.sort((a, b) => b.sortTime - a.sortTime);
  }, [activitySummary, collaborationState?.projects, collaborationState?.store.summaries, readNotificationIds, t]);
  const unreadNotificationCount = notifications.filter((notification) => notification.unread).length;

  const handleNotificationClick = (notification: AppNotification) => {
    setReadNotificationIds((current) =>
      current.includes(notification.id) ? current : [...current, notification.id]
    );
    setSelectedTaskResult(notification.taskResult);
    setShowNotifications(false);
  };

  return (
    <div className="relative flex h-screen w-full flex-col overflow-hidden bg-[#F1F5F9] font-sans text-slate-900 md:flex-row">
      <Sidebar 
        activeView={activeView}
        currentFilter={currentFilter}
        onSelectFilter={(f) => { setActiveView('sessions'); setCurrentFilter(f); setSelectedIds(new Set()); setFocusedId(null); }}
        onSelectCollaboration={() => { setActiveView('collaboration'); setFocusedId(null); }}
        counts={counts}
      />

      <div className="absolute top-3 right-4 z-50 flex items-center md:right-6">
        <button
          type="button"
          onClick={() => setShowNotifications((current) => !current)}
          className="relative rounded-full border border-slate-200 bg-white p-2 text-slate-500 shadow-sm transition-colors hover:bg-slate-100 hover:text-slate-700"
          title={t('notifications')}
          aria-label={t('notifications')}
        >
          <Bell className="h-5 w-5" />
          {unreadNotificationCount > 0 && (
            <span className="absolute top-1 right-1 h-2.5 w-2.5 rounded-full border-2 border-white bg-red-500"></span>
          )}
        </button>

        {showNotifications && (
          <div className="absolute top-full right-0 mt-2 w-80 overflow-hidden rounded-xl border border-slate-200 bg-white shadow-xl">
            <div className="flex items-center justify-between border-b border-slate-100 bg-slate-50 px-4 py-3">
              <h3 className="text-sm font-bold text-slate-800">{t('notifications')}</h3>
              {unreadNotificationCount > 0 && (
                <span className="rounded-full bg-blue-50 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-blue-600">
                  {unreadNotificationCount} {t('notification_new')}
                </span>
              )}
            </div>
            <div className="max-h-[300px] overflow-y-auto">
              {notifications.length > 0 ? (
                notifications.map((notification) => (
                  <button
                    key={notification.id}
                    type="button"
                    onClick={() => handleNotificationClick(notification)}
                    className={cn(
                      'w-full border-b border-slate-50 p-4 text-left transition-colors last:border-0 hover:bg-slate-50',
                      notification.unread && 'bg-blue-50/30'
                    )}
                  >
                    <div className="mb-1 flex items-start justify-between gap-3">
                      <h4 className={cn('text-sm font-semibold', notification.unread ? 'text-slate-900' : 'text-slate-700')}>
                        {notification.title}
                      </h4>
                      <span className="shrink-0 font-mono text-[10px] text-slate-400">{notification.time}</span>
                    </div>
                    <p className="text-xs leading-relaxed text-slate-500">{notification.message}</p>
                  </button>
                ))
              ) : (
                <div className="px-4 py-8 text-center text-sm text-slate-400">
                  {t('notification_empty')}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {activeView === 'sessions' ? (
        <>
          <MainList
            sessions={filteredSessions}
            selectedIds={selectedIds}
            focusedId={focusedId}
            onToggleSelect={handleToggleSelect}
            onFocus={setFocusedId}
            scopeKey={explorerScopeKey}
            searchQuery={searchQuery}
            onSearchChange={setSearchQuery}
            currentFilterText={filterTitle}
            isStaleFilterActive={currentFilter === 'stale'}
            staleSessionCount={counts.stale}
            onToggleStaleFilter={() => {
              setActiveView('sessions');
              setCurrentFilter(currentFilter === 'stale' ? 'all' : 'stale');
              setSelectedIds(new Set());
              setFocusedId(null);
            }}
            workspacePath={workspacePath}
            onWorkspacePathChange={setWorkspacePath}
            onScan={handleScan}
            staleAfterDays={staleAfterDays}
            staleAfterDaysDraft={staleAfterDaysDraft}
            onStaleAfterDaysDraftChange={setStaleAfterDaysDraft}
            onSaveStaleAfterDays={handleSaveStaleAfterDays}
            isSettingsSaving={isSettingsSaving}
            isLoading={isLoading}
            errorMessage={errorMessage}
            noticeMessage={noticeMessage}
            activitySummary={activitySummary}
            summaryDays={summaryDays}
            onSummaryDaysChange={setSummaryDays}
            isGeneratingSummary={isGeneratingSummary}
            onGenerateActivitySummary={handleGenerateActivitySummary}
            onClearActivitySummary={() => setActivitySummary(null)}
            sortField={sortField}
            sortDirection={sortDirection}
            onSortChange={handleSortChange}
            selectedLabelFilters={selectedLabelFilters}
            onToggleLabelFilter={handleToggleLabelFilter}
            onClearLabelFilters={() => setSelectedLabelFilters([])}
            showBulkActions={selectedIds.size > 0 && currentFilter !== 'deleted'}
            selectedCount={selectedIds.size}
            isBulkActionBusy={isMutating}
            onBulkArchiveDelete={handleBulkArchiveDelete}
            onClearSelection={() => setSelectedIds(new Set())}
            collaborationProjects={collaborationState?.projects ?? []}
            sharePolicies={collaborationState?.store.projectPolicies ?? []}
            isSavingSharePolicy={isSavingSharePolicy}
            onToggleProjectShare={handleToggleSharePolicy}
          />

          {focusedSession && (
            <PreviewPanel
              session={focusedSession}
              onClose={() => setFocusedId(null)}
              onDelete={handleDelete}
              onRestore={handleRestore}
              onUpdateLabels={handleUpdateLabels}
              onUpdateNotes={handleUpdateNotes}
              onLoadContent={fetchSessionContent}
              onUpdateContent={handleUpdateContent}
              isBusy={isMutating}
            />
          )}
        </>
      ) : (
        <CollaborationPanel
          layout="page"
          state={collaborationState}
          peerBaseUrl={peerBaseUrl}
          onPeerBaseUrlChange={setPeerBaseUrl}
          peerAccessToken={peerAccessToken}
          onPeerAccessTokenChange={setPeerAccessToken}
          selectedPeerId={selectedPeerId}
          onSelectedPeerIdChange={handleSelectPeer}
          peerProjects={peerProjects}
          selectedProjectId={collaborationProjectId}
          onSelectedProjectIdChange={setCollaborationProjectId}
          summaryDays={collaborationSummaryDays}
          onSummaryDaysChange={setCollaborationSummaryDays}
          isLoading={isCollaborationLoading}
          isPairingPeer={isPairingPeer}
          isLoadingPeerProjects={isLoadingPeerProjects}
          isUpdatingPeerToken={isUpdatingPeerToken}
          isDeletingPeer={isDeletingPeer}
          isRefreshingLocalToken={isRefreshingLocalToken}
          generatingProjectIds={generatingCollaborationProjectIds}
          isRefreshingIncremental={isRefreshingCollaboration}
          onRefresh={loadCollaborationState}
          onLocalDisplayNameChange={handleUpdateLocalDisplayName}
          onRefreshLocalPeerToken={handleRefreshLocalPeerToken}
          onUpdatePeerConnection={handleUpdatePeerConnection}
          onDeletePeer={handleDeleteTrustedPeer}
          onPairPeer={handlePairPeer}
          onUseDiscoveredPeer={handleUseDiscoveredPeer}
          onCreateSubscription={handleCreateCollaborationSubscription}
          onGenerateIncremental={handleGenerateCollaborationIncremental}
          onUpdateSubscriptionSchedule={handleUpdateSubscriptionSchedule}
          latestSummary={latestCollaborationSummary}
          errorMessage={collaborationErrorMessage}
          noticeMessage={collaborationNoticeMessage}
        />
      )}

      {selectedTaskResult && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 p-4 backdrop-blur-sm"
          onClick={() => setSelectedTaskResult(null)}
        >
          <div
            className="flex max-h-[84vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl bg-white shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-slate-200 bg-slate-50 px-5 py-4">
              <h3 className="flex items-center gap-2 text-lg font-bold text-slate-800">
                <CheckCircle className={cn('h-5 w-5', selectedTaskResult.status === 'success' ? 'text-emerald-500' : 'text-red-500')} />
                {t('collab_task_detail_title')}
              </h3>
              <button
                type="button"
                onClick={() => setSelectedTaskResult(null)}
                className="rounded-md p-1.5 text-slate-400 transition-colors hover:bg-slate-200 hover:text-slate-600"
                title={t('collab_close')}
                aria-label={t('collab_close')}
              >
                <X className="h-5 w-5" />
              </button>
            </div>
            <div className="border-b border-slate-100 bg-slate-50/50 px-6 py-3">
              <div className="flex flex-wrap items-center gap-6 text-sm">
                <div>
                  <span className="mr-2 font-medium text-slate-400">{t('collab_task_time')}:</span>
                  <span className="font-mono text-slate-700">{selectedTaskResult.time}</span>
                </div>
                <div>
                  <span className="mr-2 font-medium text-slate-400">{t('collab_task_status')}:</span>
                  <span
                    className={cn(
                      'inline-flex items-center gap-1.5 rounded-sm px-2 py-0.5 text-xs font-bold uppercase tracking-wider',
                      selectedTaskResult.status === 'success' ? 'bg-emerald-100 text-emerald-700' : 'bg-red-100 text-red-700'
                    )}
                  >
                    {selectedTaskResult.status === 'success' ? t('collab_status_success') : t('collab_status_failed')}
                  </span>
                </div>
              </div>
            </div>
            <div className="overflow-y-auto bg-white p-6">
              <pre className="whitespace-pre-wrap font-mono text-sm font-medium leading-relaxed text-slate-700">
                {selectedTaskResult.result}
              </pre>
            </div>
            <div className="flex justify-end border-t border-slate-100 bg-slate-50 px-6 py-4">
              <button
                type="button"
                onClick={() => setSelectedTaskResult(null)}
                className="rounded-lg border border-slate-300 bg-white px-5 py-2 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:bg-slate-50"
              >
                {t('collab_close')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
