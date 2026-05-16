import React, { useEffect, useMemo, useState } from 'react';
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
import { subDays } from 'date-fns';
import { useI18n } from './i18n';
import {
  archiveDeleteSession,
  createCollaborationSubscription,
  fetchSessions,
  fetchCollaborationState,
  generateCollaborationIncremental,
  generateActivitySummary,
  pairPeer,
  restoreSession,
  scanSessions,
  updateSharePolicy,
  updateSessionLabels,
  updateSessionNotes,
  updateSettings,
} from './api';

const DEFAULT_STALE_AFTER_DAYS = 15;

function normalizePeerBaseUrl(value: string) {
  const trimmed = value.trim().replace(/\/+$/, '');
  if (!trimmed) return '';
  return trimmed.includes('://') ? trimmed : `http://${trimmed}`;
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
  const [peerProjects, setPeerProjects] = useState<PeerProject[]>([]);
  const [collaborationProjectId, setCollaborationProjectId] = useState('');
  const [collaborationSummaryDays, setCollaborationSummaryDays] = useState(7);
  const [isCollaborationLoading, setIsCollaborationLoading] = useState(false);
  const [isPairingPeer, setIsPairingPeer] = useState(false);
  const [isSavingSharePolicy, setIsSavingSharePolicy] = useState(false);
  const [isGeneratingCollaboration, setIsGeneratingCollaboration] = useState(false);
  const [isRefreshingCollaboration, setIsRefreshingCollaboration] = useState(false);
  const [latestCollaborationSummary, setLatestCollaborationSummary] = useState<CollaborationSummary | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [noticeMessage, setNoticeMessage] = useState<string | null>(null);

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

  const loadCollaborationState = async () => {
    setIsCollaborationLoading(true);
    try {
      const response = await fetchCollaborationState();
      setCollaborationState(response);
      setCollaborationProjectId((current) => current || response.projects[0]?.projectId || '');
      setSelectedPeerId((current) => current || response.store.trustedPeers[0]?.peerId || '');
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_collaboration_load_failed'));
    } finally {
      setIsCollaborationLoading(false);
    }
  };

  useEffect(() => {
    void loadCollaborationState();
  }, []);

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
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await updateSharePolicy(projectId, {
        projectPath,
        enabled,
        sharedLabels: ['share', 'team', 'review', 'collab'],
        blockedLabels: ['private', 'secret'],
      });
      setCollaborationState(response);
      setNoticeMessage(t('collab_share_policy_saved'));
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_collaboration_save_failed'));
    } finally {
      setIsSavingSharePolicy(false);
    }
  };

  const handlePairPeer = async () => {
    if (!peerBaseUrl.trim()) return;

    setIsPairingPeer(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const normalizedPeerBaseUrl = normalizePeerBaseUrl(peerBaseUrl);
      setPeerBaseUrl(normalizedPeerBaseUrl);
      const response = await pairPeer({
        peerBaseUrl: normalizedPeerBaseUrl,
        peerAccessToken: peerAccessToken.trim() || undefined,
      });
      setCollaborationState(response.state);
      setPeerProjects(response.peerProjects);
      setSelectedPeerId(response.peer.peerId);
      setNoticeMessage(t('collab_peer_paired'));
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_collaboration_pair_failed'));
    } finally {
      setIsPairingPeer(false);
    }
  };

  const handleUseDiscoveredPeer = (peer: PeerPresence) => {
    setPeerBaseUrl(peer.baseUrl);
    setSelectedPeerId('');
    setPeerProjects([]);
  };

  const handleCreateCollaborationSubscription = async () => {
    if (!collaborationProjectId || (!selectedPeerId && !peerBaseUrl.trim())) return;

    setIsGeneratingCollaboration(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const normalizedPeerBaseUrl = normalizePeerBaseUrl(peerBaseUrl);
      if (!selectedPeerId) {
        setPeerBaseUrl(normalizedPeerBaseUrl);
      }
      const response = await createCollaborationSubscription({
        peerId: selectedPeerId || undefined,
        peerBaseUrl: selectedPeerId ? undefined : normalizedPeerBaseUrl,
        peerAccessToken: peerAccessToken.trim() || undefined,
        projectId: collaborationProjectId,
        days: collaborationSummaryDays,
        language: lang,
      });
      setCollaborationState(response.state);
      setLatestCollaborationSummary(response.summary);
      setSelectedPeerId(response.subscription.peerId);
      setNoticeMessage(t('collab_subscription_created'));
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_collaboration_summary_failed'));
    } finally {
      setIsGeneratingCollaboration(false);
    }
  };

  const handleGenerateCollaborationIncremental = async () => {
    const subscription = collaborationState?.store.subscriptions.find(
      (item) => item.peerId === selectedPeerId && item.projectId === collaborationProjectId && item.status === 'active'
    );
    if (!subscription) return;

    setIsRefreshingCollaboration(true);
    setErrorMessage(null);
    setNoticeMessage(null);
    try {
      const response = await generateCollaborationIncremental({
        subscriptionId: subscription.subscriptionId,
        peerAccessToken: peerAccessToken.trim() || undefined,
        language: lang,
      });
      setCollaborationState(response.state);
      setLatestCollaborationSummary(response.summary);
      setNoticeMessage(t('collab_incremental_created'));
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('error_collaboration_incremental_failed'));
    } finally {
      setIsRefreshingCollaboration(false);
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

  return (
    <div className="flex h-screen w-full flex-col overflow-hidden bg-[#F1F5F9] font-sans text-slate-900 md:flex-row">
      <Sidebar 
        activeView={activeView}
        currentFilter={currentFilter}
        onSelectFilter={(f) => { setActiveView('sessions'); setCurrentFilter(f); setSelectedIds(new Set()); setFocusedId(null); }}
        onSelectCollaboration={() => { setActiveView('collaboration'); setFocusedId(null); }}
        counts={counts}
      />

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
          onSelectedPeerIdChange={setSelectedPeerId}
          peerProjects={peerProjects}
          selectedProjectId={collaborationProjectId}
          onSelectedProjectIdChange={setCollaborationProjectId}
          summaryDays={collaborationSummaryDays}
          onSummaryDaysChange={setCollaborationSummaryDays}
          isLoading={isCollaborationLoading}
          isPairingPeer={isPairingPeer}
          isGenerating={isGeneratingCollaboration}
          isRefreshingIncremental={isRefreshingCollaboration}
          onRefresh={loadCollaborationState}
          onPairPeer={handlePairPeer}
          onUseDiscoveredPeer={handleUseDiscoveredPeer}
          onCreateSubscription={handleCreateCollaborationSubscription}
          onGenerateIncremental={handleGenerateCollaborationIncremental}
          latestSummary={latestCollaborationSummary}
          errorMessage={errorMessage}
          noticeMessage={noticeMessage}
        />
      )}
    </div>
  );
}
