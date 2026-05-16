import React, { useMemo, useState } from 'react';
import {
  ArrowLeft,
  Check,
  CheckCircle,
  CheckCircle2,
  ChevronRight,
  Clipboard,
  Clock,
  RefreshCw,
  Settings,
  ShieldCheck,
  UserPlus,
  Users,
  Wifi,
  XCircle,
} from 'lucide-react';
import {
  CollaborationStateResponse,
  CollaborationSummary,
  PeerPresence,
  PeerProject,
} from '../types';
import { useI18n } from '../i18n';
import { cn } from '../lib/utils';

interface CollaborationPanelProps {
  layout?: 'sidebar' | 'page';
  state: CollaborationStateResponse | null;
  peerBaseUrl: string;
  onPeerBaseUrlChange: (value: string) => void;
  peerAccessToken: string;
  onPeerAccessTokenChange: (value: string) => void;
  selectedPeerId: string;
  onSelectedPeerIdChange: (value: string) => void;
  peerProjects: PeerProject[];
  selectedProjectId: string;
  onSelectedProjectIdChange: (value: string) => void;
  summaryDays: number;
  onSummaryDaysChange: (value: number) => void;
  isLoading: boolean;
  isPairingPeer: boolean;
  isLoadingPeerProjects: boolean;
  isGenerating: boolean;
  isRefreshingIncremental: boolean;
  onRefresh: () => void;
  onPairPeer: () => void;
  onUseDiscoveredPeer: (peer: PeerPresence) => void;
  onCreateSubscription: (projectId?: string) => void;
  onGenerateIncremental: () => void;
  latestSummary: CollaborationSummary | null;
  errorMessage?: string | null;
  noticeMessage?: string | null;
}

interface LocalConfigValueProps {
  label: string;
  value?: string | null;
  fallback?: string;
  copied: boolean;
  onCopy?: () => void;
}

function LocalConfigValue({ label, value, fallback = '-', copied, onCopy }: LocalConfigValueProps) {
  const { t } = useI18n();
  const displayValue = value || fallback;

  return (
    <div className="min-w-0">
      <label className="mb-1 block text-[10px] font-bold uppercase tracking-wider text-slate-400">{label}</label>
      <div className="flex min-h-8 items-center gap-2 rounded border border-slate-100 bg-slate-50 px-2 py-1.5">
        <div className="min-w-0 flex-1 truncate font-mono text-xs font-medium text-slate-600" title={displayValue}>
          {displayValue}
        </div>
        {value && onCopy && (
          <button
            type="button"
            title={copied ? t('btn_copy_copied') : t('btn_copy_value')}
            onClick={onCopy}
            className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded text-slate-500 transition-colors hover:bg-white hover:text-slate-800"
          >
            {copied ? <Check className="h-3.5 w-3.5" /> : <Clipboard className="h-3.5 w-3.5" />}
          </button>
        )}
      </div>
    </div>
  );
}

export function CollaborationPanel({
  layout = 'sidebar',
  state,
  peerBaseUrl,
  onPeerBaseUrlChange,
  peerAccessToken,
  onPeerAccessTokenChange,
  selectedPeerId,
  onSelectedPeerIdChange,
  peerProjects,
  selectedProjectId,
  onSelectedProjectIdChange,
  summaryDays,
  onSummaryDaysChange,
  isLoading,
  isPairingPeer,
  isLoadingPeerProjects,
  isGenerating,
  isRefreshingIncremental,
  onRefresh,
  onPairPeer,
  onUseDiscoveredPeer,
  onCreateSubscription,
  onGenerateIncremental,
  latestSummary,
  errorMessage,
  noticeMessage,
}: CollaborationPanelProps) {
  const { t } = useI18n();
  const [copiedLocalField, setCopiedLocalField] = useState<string | null>(null);
  const [expandedProjectId, setExpandedProjectId] = useState<string | null>(null);
  const [selectedDetailProjectId, setSelectedDetailProjectId] = useState<string | null>(null);
  const [detailTab, setDetailTab] = useState<'config' | 'tasks'>('config');
  const [analysisPrompt, setAnalysisPrompt] = useState('');
  const [analysisCycle, setAnalysisCycle] = useState('1h');
  const peers = state?.store.trustedPeers ?? [];
  const discoveredPeers = state?.discoveredPeers ?? [];
  const summaries = state?.store.summaries ?? [];
  const localConfig = state?.localConfig;
  const selectedPeer = peers.find((peer) => peer.peerId === selectedPeerId) ?? null;
  const selectedDetailProject = selectedDetailProjectId
    ? peerProjects.find((project) => project.projectId === selectedDetailProjectId) ?? null
    : null;
  const activeSummaryProjectId = selectedDetailProjectId ?? selectedProjectId;
  const activeSummary = useMemo(() => {
    if (!activeSummaryProjectId) return null;

    return [latestSummary, ...summaries]
      .filter((summary): summary is CollaborationSummary => Boolean(summary))
      .filter(
        (summary) =>
          summary.projectId === activeSummaryProjectId &&
          (!selectedPeerId || summary.sourceIds.includes(selectedPeerId))
      )
      .sort((a, b) => new Date(b.generatedAt).getTime() - new Date(a.generatedAt).getTime())[0] ?? null;
  }, [activeSummaryProjectId, latestSummary, selectedPeerId, summaries]);
  const activeSubscription = state?.store.subscriptions.find(
    (subscription) =>
      subscription.peerId === selectedPeerId &&
      subscription.projectId === selectedProjectId &&
      subscription.status === 'active'
  );
  const canPair = Boolean(peerBaseUrl.trim()) && !isPairingPeer;
  const canSubscribe = Boolean(selectedPeerId && selectedProjectId) && !isGenerating;
  const copyLocalConfigValue = async (field: string, value?: string | null) => {
    if (!value) return;
    await navigator.clipboard.writeText(value);
    setCopiedLocalField(field);
    window.setTimeout(() => setCopiedLocalField((current) => (current === field ? null : current)), 1500);
  };
  const peerCards = useMemo(
    () => [
      ...peers.map((peer) => ({ kind: 'paired' as const, id: peer.peerId, label: peer.displayName, baseUrl: peer.baseUrl })),
      ...discoveredPeers
        .filter((peer) => !peers.some((trusted) => trusted.peerId === peer.peerId))
        .map((peer) => ({ kind: 'discovered' as const, id: peer.peerId, label: peer.displayName, baseUrl: peer.baseUrl, peer })),
    ],
    [discoveredPeers, peers]
  );

  const choosePeer = (card: (typeof peerCards)[number]) => {
    if (card.kind === 'paired') {
      onSelectedPeerIdChange(card.id);
      setSelectedDetailProjectId(null);
      return;
    }

    onUseDiscoveredPeer(card.peer);
    setSelectedDetailProjectId(null);
  };

  const chooseProject = (project: PeerProject) => {
    setExpandedProjectId((current) => (current === project.projectId ? null : project.projectId));
    onSelectedProjectIdChange(project.projectId);
  };

  const openProjectDetail = (project: PeerProject) => {
    onSelectedProjectIdChange(project.projectId);
    setSelectedDetailProjectId(project.projectId);
    setDetailTab('config');
  };

  return (
    <main
      className={cn(
        'flex w-full min-w-0 flex-col overflow-hidden bg-[#F8FAFC] font-sans text-slate-900',
        layout === 'page' ? 'flex-1' : 'max-h-[38vh] border-t border-slate-200 xl:h-screen xl:max-h-none xl:w-80 xl:border-l xl:border-t-0'
      )}
    >
      <header className="flex h-16 flex-shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 md:px-6">
        <h2 className="flex items-center gap-2 text-lg font-semibold tracking-tight text-slate-800">
          <Users className="h-5 w-5 text-blue-500" />
          {t('collab_title')}
        </h2>
        <button
          type="button"
          title={t('btn_refresh_collaboration')}
          onClick={onRefresh}
          disabled={isLoading}
          className="rounded border border-slate-200 p-1.5 text-slate-500 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
        >
          <RefreshCw className={cn('h-4 w-4', isLoading && 'animate-spin')} />
        </button>
      </header>

      <div className="flex-1 overflow-y-auto p-4 md:p-8">
        <div className="mx-auto max-w-5xl space-y-6">
          {errorMessage && (
            <div className="rounded border border-red-200 bg-red-50 px-4 py-3 text-sm font-medium text-red-700">
              {errorMessage}
            </div>
          )}
          {noticeMessage && (
            <div className="rounded border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm font-medium text-emerald-700">
              {noticeMessage}
            </div>
          )}

          <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
            <div className="grid gap-4 md:grid-cols-[1fr_auto] md:items-center">
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                <LocalConfigValue
                  label={t('collab_local_display_name')}
                  value={localConfig?.displayName}
                  copied={copiedLocalField === 'displayName'}
                  onCopy={() => copyLocalConfigValue('displayName', localConfig?.displayName)}
                />
                <LocalConfigValue
                  label={t('collab_local_base_url')}
                  value={localConfig?.baseUrl}
                  copied={copiedLocalField === 'baseUrl'}
                  onCopy={() => copyLocalConfigValue('baseUrl', localConfig?.baseUrl)}
                />
                <LocalConfigValue
                  label={t('collab_local_token')}
                  value={localConfig?.peerToken}
                  fallback={t('collab_token_missing')}
                  copied={copiedLocalField === 'peerToken'}
                  onCopy={() => copyLocalConfigValue('peerToken', localConfig?.peerToken)}
                />
              </div>
              <span className="inline-flex w-fit items-center gap-2 rounded-full border border-emerald-100 bg-emerald-50 px-3 py-1.5 text-xs font-bold uppercase tracking-wide text-emerald-600">
                <span className="h-2 w-2 rounded-full bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)]"></span>
                {localConfig?.lanDiscoveryEnabled ? t('collab_discovering') : t('collab_disabled')}
              </span>
            </div>
          </section>

          <div className="grid grid-cols-1 items-start gap-6 md:grid-cols-[280px_1fr]">
            <aside className="space-y-4">
              <h3 className="flex items-center gap-2 px-1 text-sm font-bold uppercase tracking-wider text-slate-400">
                <Users className="h-4 w-4" />
                {t('collab_peers')}
              </h3>
              <div className="space-y-3">
                {peerCards.map((card) => {
                  const isSelected = card.kind === 'paired' ? selectedPeerId === card.id : peerBaseUrl === card.baseUrl;
                  const projectCount = card.kind === 'paired' && card.id === selectedPeerId ? peerProjects.length : 0;
                  return (
                    <button
                      key={`${card.kind}:${card.id}`}
                      type="button"
                      onClick={() => choosePeer(card)}
                      className={cn(
                        'w-full rounded-xl border bg-white p-4 text-left shadow-sm transition-all',
                        isSelected ? 'border-blue-500 bg-blue-50/10 ring-1 ring-blue-500' : 'border-slate-200 hover:border-blue-300 hover:bg-slate-50'
                      )}
                    >
                      <div className="mb-2 flex items-start justify-between gap-2">
                        <h4 className="flex min-w-0 items-center gap-2 font-bold text-slate-800">
                          <span
                            className={cn(
                              'h-2 w-2 flex-shrink-0 rounded-full',
                              card.kind === 'paired' ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.8)]' : 'bg-slate-300'
                            )}
                          ></span>
                          <span className="truncate">{card.label}</span>
                        </h4>
                        <span
                          className={cn(
                            'flex-shrink-0 rounded-full px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider',
                            card.kind === 'paired' ? 'bg-blue-50 text-blue-600' : 'bg-slate-100 text-slate-500'
                          )}
                        >
                          {card.kind === 'paired' ? t('collab_status_paired') : t('collab_status_discovered')}
                        </span>
                      </div>
                      <p className="truncate font-mono text-xs text-slate-500">{card.baseUrl || '-'}</p>
                      {card.kind === 'paired' && (
                        <p className="mt-2 text-xs font-medium text-slate-500">
                          {isSelected && isLoadingPeerProjects
                            ? t('collab_loading_projects')
                            : t('collab_exposed_project_count', { count: projectCount })}
                        </p>
                      )}
                    </button>
                  );
                })}
                {peerCards.length === 0 && (
                  <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50 p-5 text-sm font-medium text-slate-400">
                    {t('collab_no_peers')}
                  </div>
                )}
                <div className="space-y-2 rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
                  <input
                    value={peerBaseUrl}
                    onChange={(event) => onPeerBaseUrlChange(event.target.value)}
                    placeholder="http://192.168.1.12:4000"
                    className="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                  />
                  <input
                    value={peerAccessToken}
                    onChange={(event) => onPeerAccessTokenChange(event.target.value)}
                    placeholder={t('collab_peer_token')}
                    type="password"
                    className="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                  />
                  <button
                    type="button"
                    onClick={onPairPeer}
                    disabled={!canPair}
                    className="inline-flex w-full items-center justify-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-bold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-blue-300"
                  >
                    <UserPlus className="h-4 w-4" />
                    {isPairingPeer ? t('collab_pairing') : t('collab_btn_pair')}
                  </button>
                </div>
              </div>
            </aside>

            <section className="space-y-4">
              {selectedDetailProject ? (
                <div className="flex min-h-[500px] flex-col overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
                  <div className="border-b border-slate-100 bg-slate-50 px-5 py-4">
                    <button
                      type="button"
                      onClick={() => setSelectedDetailProjectId(null)}
                      className="mb-4 flex items-center gap-1.5 text-xs font-bold uppercase tracking-wider text-slate-500 transition-colors hover:text-slate-800"
                    >
                      <ArrowLeft className="h-4 w-4" />
                      {t('collab_back')}
                    </button>
                    <div className="flex items-start gap-4">
                      <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded bg-blue-600 text-white shadow-sm">
                        <Settings className="h-5 w-5" />
                      </div>
                      <div className="min-w-0">
                        <h3 className="truncate text-lg font-bold leading-tight tracking-tight text-slate-800">
                          {selectedDetailProject.pathLabel}
                        </h3>
                        <p className="mt-1 font-mono text-xs text-slate-500">
                          {t('collab_remote_sessions', { count: selectedDetailProject.activeSessionCount })}
                        </p>
                      </div>
                    </div>
                  </div>

                  <div className="flex border-b border-slate-200 bg-slate-50 px-2">
                    <button
                      type="button"
                      onClick={() => setDetailTab('config')}
                      className={cn(
                        'border-b-2 px-4 py-2.5 text-sm font-bold uppercase tracking-wider transition-colors',
                        detailTab === 'config' ? 'border-blue-600 text-blue-600' : 'border-transparent text-slate-500 hover:text-slate-800'
                      )}
                    >
                      {t('collab_tab_config')}
                    </button>
                    <button
                      type="button"
                      onClick={() => setDetailTab('tasks')}
                      className={cn(
                        'border-b-2 px-4 py-2.5 text-sm font-bold uppercase tracking-wider transition-colors',
                        detailTab === 'tasks' ? 'border-blue-600 text-blue-600' : 'border-transparent text-slate-500 hover:text-slate-800'
                      )}
                    >
                      {t('collab_tab_tasks')}
                    </button>
                  </div>

                  <div className="flex-1 bg-white p-6">
                    {detailTab === 'config' ? (
                      <div className="max-w-2xl space-y-6">
                        <div className="space-y-2">
                          <label className="block text-sm font-bold text-slate-800">{t('collab_analysis_prompt')}</label>
                          <textarea
                            value={analysisPrompt}
                            onChange={(event) => setAnalysisPrompt(event.target.value)}
                            placeholder={t('collab_analysis_prompt_placeholder')}
                            className="h-32 w-full resize-none rounded-lg border border-slate-300 p-3 font-mono text-sm shadow-sm outline-none transition-shadow focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                          />
                        </div>
                        <div className="space-y-2">
                          <label className="block text-sm font-bold text-slate-800">{t('collab_analysis_cycle')}</label>
                          <div className="grid gap-3 sm:grid-cols-3">
                            {[
                              { id: '10m', label: t('collab_cycle_10m') },
                              { id: '1h', label: t('collab_cycle_1h') },
                              { id: 'manual', label: t('collab_cycle_manual') },
                            ].map((cycle) => (
                              <button
                                key={cycle.id}
                                type="button"
                                onClick={() => setAnalysisCycle(cycle.id)}
                                className={cn(
                                  'flex justify-center rounded-lg border px-4 py-3 text-sm font-semibold transition-all',
                                  analysisCycle === cycle.id
                                    ? 'border-blue-600 bg-blue-50 text-blue-700 ring-1 ring-blue-600'
                                    : 'border-slate-200 bg-white text-slate-600 hover:border-slate-300 hover:bg-slate-50'
                                )}
                              >
                                {cycle.label}
                              </button>
                            ))}
                          </div>
                        </div>
                        <div className="flex flex-wrap justify-end gap-2 border-t border-slate-100 pt-4">
                          <input
                            type="number"
                            min={1}
                            max={90}
                            value={summaryDays}
                            onChange={(event) => onSummaryDaysChange(Number(event.target.value))}
                            className="w-20 rounded border border-slate-300 px-2 py-2 text-sm outline-none focus:border-blue-500"
                            aria-label={t('summary_range_label')}
                          />
                          <button
                            type="button"
                            onClick={() => onCreateSubscription()}
                            disabled={!canSubscribe}
                            className="inline-flex items-center gap-2 rounded-lg bg-blue-600 px-5 py-2.5 text-sm font-bold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-blue-300"
                          >
                            <Check className="h-4 w-4" />
                            {isGenerating ? t('collab_generating') : t('collab_subscribe_baseline')}
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="overflow-x-auto">
                        <table className="w-full border-collapse text-left">
                          <thead>
                            <tr className="border-b border-slate-200 text-xs font-bold uppercase tracking-wider text-slate-400">
                              <th className="px-4 py-3">{t('collab_task_time')}</th>
                              <th className="px-4 py-3">{t('collab_task_status')}</th>
                              <th className="px-4 py-3">{t('collab_task_result')}</th>
                            </tr>
                          </thead>
                          <tbody className="text-sm">
                            {isRefreshingIncremental && (
                              <tr className="border-b border-slate-100/50">
                                <td className="whitespace-nowrap px-4 py-4 font-mono text-xs text-slate-600">
                                  <Clock className="mr-1.5 inline h-3.5 w-3.5 text-slate-400" />
                                  {t('collab_now')}
                                </td>
                                <td className="whitespace-nowrap px-4 py-4">
                                  <span className="inline-flex items-center gap-1.5 rounded-full border border-amber-100 bg-amber-50 px-2.5 py-1 text-xs font-bold uppercase tracking-wider text-amber-600">
                                    <RefreshCw className="h-3 w-3 animate-spin" /> {t('collab_status_running')}
                                  </span>
                                </td>
                                <td className="max-w-xs truncate px-4 py-4 text-slate-500 italic">
                                  {t('collab_refreshing_incremental')}
                                </td>
                              </tr>
                            )}
                            {activeSummary ? (
                              <tr className="border-b border-slate-100/50 transition-colors hover:bg-slate-50/50">
                                <td className="whitespace-nowrap px-4 py-4 font-mono text-xs text-slate-600">
                                  <Clock className="mr-1.5 inline h-3.5 w-3.5 text-slate-400" />
                                  {new Date(activeSummary.generatedAt).toLocaleString()}
                                </td>
                                <td className="whitespace-nowrap px-4 py-4">
                                  <span className="inline-flex items-center gap-1.5 rounded-full border border-emerald-100 bg-emerald-50 px-2.5 py-1 text-xs font-bold uppercase tracking-wider text-emerald-600">
                                    <CheckCircle className="h-3 w-3" /> {t('collab_status_success')}
                                  </span>
                                </td>
                                <td className="max-w-md px-4 py-4 text-slate-700">
                                  <div className="max-h-40 overflow-y-auto whitespace-pre-wrap text-sm leading-6">
                                    {activeSummary.markdown}
                                  </div>
                                </td>
                              </tr>
                            ) : (
                              <tr>
                                <td colSpan={3} className="px-4 py-8 text-center text-sm text-slate-400">
                                  {t('collab_no_summary')}
                                </td>
                              </tr>
                            )}
                            {!activeSubscription && (
                              <tr>
                                <td colSpan={3} className="px-4 py-4 text-center text-xs text-slate-400">
                                  {t('collab_no_active_subscription')}
                                </td>
                              </tr>
                            )}
                          </tbody>
                        </table>
                        <div className="mt-4 flex justify-end">
                          <button
                            type="button"
                            onClick={onGenerateIncremental}
                            disabled={!activeSubscription || isRefreshingIncremental}
                            className="inline-flex items-center gap-2 rounded border border-slate-300 px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400"
                          >
                            <RefreshCw className={cn('h-4 w-4', isRefreshingIncremental && 'animate-spin')} />
                            {isRefreshingIncremental ? t('collab_refreshing_incremental') : t('collab_refresh_incremental')}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              ) : !selectedPeer && !peerBaseUrl ? (
                <div className="flex h-64 flex-col items-center justify-center rounded-xl border-2 border-dashed border-slate-200 bg-slate-50 text-slate-400">
                  <Wifi className="mb-3 h-10 w-10 opacity-20" />
                  <p className="text-sm font-medium">{t('collab_select_peer')}</p>
                </div>
              ) : !selectedPeer ? (
                <div className="flex flex-col items-center justify-center rounded-xl border border-slate-200 bg-white p-10 text-center shadow-sm">
                  <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full border border-blue-100 bg-blue-50">
                    <ShieldCheck className="h-6 w-6 text-blue-500" />
                  </div>
                  <h3 className="mb-2 text-lg font-bold text-slate-800">{t('collab_pairing_required')}</h3>
                  <div className="mb-6 grid w-full max-w-lg gap-3 sm:grid-cols-[1fr_1fr_auto]">
                    <input
                      value={peerBaseUrl}
                      onChange={(event) => onPeerBaseUrlChange(event.target.value)}
                      placeholder="http://192.168.1.12:4000"
                      className="rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                    />
                    <input
                      value={peerAccessToken}
                      onChange={(event) => onPeerAccessTokenChange(event.target.value)}
                      placeholder={t('collab_peer_token')}
                      type="password"
                      className="rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                    />
                    <button
                      type="button"
                      onClick={onPairPeer}
                      disabled={!canPair}
                      className="inline-flex items-center justify-center gap-2 rounded-lg bg-blue-600 px-5 py-2 text-sm font-bold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-blue-300"
                    >
                      <UserPlus className="h-4 w-4" />
                      {isPairingPeer ? t('collab_pairing') : t('collab_btn_pair')}
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <h3 className="flex items-center gap-2 px-1 text-sm font-bold uppercase tracking-wider text-slate-400">
                    <ShieldCheck className="h-4 w-4" />
                    {t('collab_exposed_projects')} - {selectedPeer.displayName}
                  </h3>
                  <div className="space-y-4">
                    {peerProjects.map((project) => {
                      const isExpanded = expandedProjectId === project.projectId;
                      const isSubscribed = state?.store.subscriptions.some(
                        (subscription) =>
                          subscription.peerId === selectedPeerId &&
                          subscription.projectId === project.projectId &&
                          subscription.status === 'active'
                      );
                      return (
                        <div key={project.projectId} className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
                          <button
                            type="button"
                            onClick={() => chooseProject(project)}
                            className="flex w-full items-center justify-between gap-4 border-b border-slate-100 bg-slate-50 px-5 py-4 text-left transition-colors hover:bg-slate-100/50"
                          >
                            <div className="min-w-0">
                              <h4 className="truncate text-sm font-bold text-slate-800">{project.pathLabel}</h4>
                              <p className="mt-0.5 font-mono text-xs text-slate-500">
                                {t('collab_remote_sessions', { count: project.activeSessionCount })}
                              </p>
                            </div>
                            <ChevronRight className={cn('h-5 w-5 flex-shrink-0 text-slate-400 transition-transform', isExpanded && 'rotate-90')} />
                          </button>
                          {isExpanded && (
                            <div className="flex items-start justify-between gap-4 p-5">
                              <button
                                type="button"
                                onClick={() => openProjectDetail(project)}
                                className={cn(
                                  'min-w-0 flex-1 text-left',
                                  isSubscribed && 'cursor-pointer'
                                )}
                              >
                                <p
                                  className={cn(
                                    'inline-block text-sm font-medium transition-colors',
                                    isSubscribed ? 'text-blue-600 underline decoration-blue-200 underline-offset-4 hover:text-blue-700' : 'text-slate-700'
                                  )}
                                >
                                  {t('collab_session_details')}
                                </p>
                                <div className="mt-2 flex flex-wrap gap-2">
                                  <span className="rounded border border-blue-100 bg-blue-50 px-1.5 py-0.5 text-[10px] font-bold uppercase text-blue-600">
                                    {t('collab_shared')}
                                  </span>
                                  {isSubscribed && (
                                    <span className="rounded border border-emerald-100 bg-emerald-50 px-1.5 py-0.5 text-[10px] font-bold uppercase text-emerald-600">
                                      {t('collab_subscribed')}
                                    </span>
                                  )}
                                </div>
                                <p className="mt-2 font-mono text-xs text-slate-400">
                                  {project.latestRecordAt ? new Date(project.latestRecordAt).toLocaleString() : '-'}
                                </p>
                              </button>
                              <button
                                type="button"
                                onClick={() => {
                                  onSelectedProjectIdChange(project.projectId);
                                  if (isSubscribed) {
                                    openProjectDetail(project);
                                  } else {
                                    onCreateSubscription(project.projectId);
                                  }
                                }}
                                disabled={!isSubscribed && isGenerating}
                                className={cn(
                                  'mt-1 flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-bold uppercase tracking-wider transition-colors',
                                  isSubscribed ? 'bg-slate-100 text-slate-500 hover:bg-slate-200' : 'bg-blue-600 text-white hover:bg-blue-700 disabled:bg-blue-300'
                                )}
                              >
                                {isSubscribed ? (
                                  <>
                                    <CheckCircle2 className="h-3.5 w-3.5" /> {t('collab_subscribed')}
                                  </>
                                ) : isGenerating && selectedProjectId === project.projectId ? (
                                  t('collab_generating')
                                ) : (
                                  t('collab_subscribe_session')
                                )}
                              </button>
                            </div>
                          )}
                        </div>
                      );
                    })}
                    {peerProjects.length === 0 && (
                      <div className="flex flex-col items-center justify-center rounded-xl border border-slate-200 bg-white p-10 text-center shadow-sm">
                        {isLoadingPeerProjects ? (
                          <RefreshCw className="mb-3 h-8 w-8 animate-spin text-slate-300" />
                        ) : (
                          <XCircle className="mb-3 h-8 w-8 text-slate-300" />
                        )}
                        <p className="text-sm font-medium text-slate-500">
                          {isLoadingPeerProjects ? t('collab_loading_projects') : t('collab_no_peer_projects')}
                        </p>
                      </div>
                    )}
                  </div>
                </>
              )}
            </section>
          </div>
        </div>
      </div>
    </main>
  );
}
