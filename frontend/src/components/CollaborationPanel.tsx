import React, { useMemo, useState } from 'react';
import {
  Check,
  Clipboard,
  FileText,
  Link2,
  RefreshCw,
  ShieldCheck,
  ToggleLeft,
  ToggleRight,
  UserPlus,
} from 'lucide-react';
import {
  CollaborationStateResponse,
  CollaborationSummary,
  PeerPresence,
  PeerProject,
  ProjectIdentity,
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
  isSavingPolicy: boolean;
  isGenerating: boolean;
  isRefreshingIncremental: boolean;
  onRefresh: () => void;
  onPairPeer: () => void;
  onUseDiscoveredPeer: (peer: PeerPresence) => void;
  onTogglePolicy: (projectId: string, projectPath: string | null, enabled: boolean) => void;
  onCreateSubscription: () => void;
  onGenerateIncremental: () => void;
  latestSummary: CollaborationSummary | null;
  errorMessage?: string | null;
  noticeMessage?: string | null;
}

function labelsText(labels: string[]) {
  return labels.length ? labels.join(', ') : 'share, team, review, collab';
}

function pathSegments(path: string) {
  return path.split(/[\\/]+/).filter(Boolean);
}

function projectDetail(project: ProjectIdentity) {
  return project.rootPath || project.pathLabel;
}

function projectDisplayName(project: ProjectIdentity, duplicateLabels: Set<string>) {
  if (!duplicateLabels.has(project.pathLabel)) {
    return project.pathLabel;
  }

  const segments = pathSegments(project.rootPath || '');
  if (segments.length >= 2) {
    return `${segments[segments.length - 2]}/${project.pathLabel}`;
  }

  return project.rootPath || project.pathLabel;
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
    <div className="min-w-0 space-y-1">
      <div className="text-xs font-medium uppercase text-slate-500">{label}</div>
      <div className="flex min-h-10 items-start gap-2 rounded border border-slate-200 bg-white px-2.5 py-2">
        <div className="min-w-0 flex-1 break-all font-mono text-xs leading-5 text-slate-700">{displayValue}</div>
        {value && onCopy && (
          <button
            type="button"
            title={copied ? t('btn_copy_copied') : t('btn_copy_value')}
            onClick={onCopy}
            className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800"
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
  isSavingPolicy,
  isGenerating,
  isRefreshingIncremental,
  onRefresh,
  onPairPeer,
  onUseDiscoveredPeer,
  onTogglePolicy,
  onCreateSubscription,
  onGenerateIncremental,
  latestSummary,
  errorMessage,
  noticeMessage,
}: CollaborationPanelProps) {
  const { t } = useI18n();
  const [expandedSummaryId, setExpandedSummaryId] = useState<string | null>(null);
  const [copiedLocalField, setCopiedLocalField] = useState<string | null>(null);
  const projects = state?.projects ?? [];
  const peers = state?.store.trustedPeers ?? [];
  const discoveredPeers = state?.discoveredPeers ?? [];
  const policies = state?.store.projectPolicies ?? [];
  const summaries = state?.store.summaries ?? [];
  const activeSummary = latestSummary ?? summaries[summaries.length - 1] ?? null;
  const localConfig = state?.localConfig;
  const duplicateProjectLabels = useMemo(() => {
    const counts = new Map<string, number>();
    projects.forEach((project) => {
      counts.set(project.pathLabel, (counts.get(project.pathLabel) ?? 0) + 1);
    });
    return new Set(
      Array.from(counts.entries())
        .filter(([, count]) => count > 1)
        .map(([label]) => label)
    );
  }, [projects]);
  const isLoopback = Boolean(
    localConfig?.baseUrl.match(/^https?:\/\/(127\.|localhost|0\.0\.0\.0|\[?::1\]?)/)
  );

  const selectedProject = useMemo(
    () => projects.find((project) => project.projectId === selectedProjectId) ?? projects[0],
    [projects, selectedProjectId]
  );

  const canPair = Boolean(peerBaseUrl.trim()) && !isPairingPeer;
  const canGenerate =
    Boolean(selectedProject?.projectId && (selectedPeerId || peerBaseUrl.trim())) && !isGenerating;
  const activeSubscription = state?.store.subscriptions.find(
    (subscription) =>
      subscription.peerId === selectedPeerId &&
      subscription.projectId === selectedProject?.projectId &&
      subscription.status === 'active'
  );
  const copyLocalConfigValue = async (field: string, value?: string | null) => {
    if (!value) return;
    await navigator.clipboard.writeText(value);
    setCopiedLocalField(field);
    window.setTimeout(() => setCopiedLocalField((current) => (current === field ? null : current)), 1500);
  };

  return (
    <main
      className={cn(
        'flex w-full flex-shrink-0 flex-col bg-white text-slate-900',
        layout === 'page'
          ? 'min-w-0 flex-1 overflow-hidden'
          : 'max-h-[38vh] border-t border-slate-200 xl:h-screen xl:max-h-none xl:w-80 xl:border-l xl:border-t-0'
      )}
    >
      <header className="flex items-center justify-between border-b border-slate-200 px-4 py-4 md:px-6">
        <div className="flex min-w-0 items-center gap-2">
          <ShieldCheck className="h-4 w-4 flex-shrink-0 text-emerald-600" />
          <h2 className="truncate text-sm font-semibold">{t('collab_title')}</h2>
        </div>
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

      <div className={cn('flex-1 overflow-y-auto p-4 md:p-6', layout === 'page' ? 'space-y-6' : 'space-y-5')}>
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

        <section className={cn('space-y-3', layout === 'page' && 'max-w-4xl')}>
          <div className="flex items-center gap-2 text-xs font-semibold uppercase text-slate-500">
            <ShieldCheck className="h-3.5 w-3.5" />
            {t('collab_local_peer')}
          </div>
          <div className="grid gap-3 rounded border border-slate-200 bg-slate-50 p-4 text-sm md:grid-cols-2">
            <LocalConfigValue
              label={t('collab_local_display_name')}
              value={localConfig?.displayName}
              copied={copiedLocalField === 'displayName'}
              onCopy={() => copyLocalConfigValue('displayName', localConfig?.displayName)}
            />
            <LocalConfigValue
              label={t('collab_local_peer_id')}
              value={localConfig?.peerId}
              copied={copiedLocalField === 'peerId'}
              onCopy={() => copyLocalConfigValue('peerId', localConfig?.peerId)}
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
            <LocalConfigValue
              label={t('collab_local_bind')}
              value={localConfig?.bindAddress}
              copied={copiedLocalField === 'bindAddress'}
              onCopy={() => copyLocalConfigValue('bindAddress', localConfig?.bindAddress)}
            />
            <div className="min-w-0 space-y-1">
              <div className="text-xs font-medium uppercase text-slate-500">{t('collab_local_discovery')}</div>
              <div className="flex min-h-10 items-center rounded border border-slate-200 bg-white px-2.5 py-2 text-slate-700">
                {localConfig?.lanDiscoveryEnabled ? t('collab_enabled') : t('collab_disabled')}
              </div>
            </div>
          </div>
          {isLoopback && (
            <div className="rounded border border-amber-200 bg-amber-50 px-3 py-2 text-xs font-medium text-amber-700">
              {t('collab_loopback_warning')}
            </div>
          )}
        </section>

        <section className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase text-slate-500">
            <Link2 className="h-3.5 w-3.5" />
            {t('collab_peer')}
          </div>
          <input
            value={peerBaseUrl}
            onChange={(event) => onPeerBaseUrlChange(event.target.value)}
            placeholder="http://192.168.1.12:4000"
            className="w-full rounded border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500"
          />
          <input
            value={peerAccessToken}
            onChange={(event) => onPeerAccessTokenChange(event.target.value)}
            placeholder={t('collab_peer_token')}
            type="password"
            className="w-full rounded border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500"
          />
          <button
            type="button"
            onClick={onPairPeer}
            disabled={!canPair}
            className="flex w-full items-center justify-center gap-2 rounded border border-slate-300 px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400"
          >
            <UserPlus className="h-4 w-4" />
            {isPairingPeer ? t('collab_pairing') : t('collab_pair_peer')}
          </button>
          <select
            value={selectedPeerId}
            onChange={(event) => onSelectedPeerIdChange(event.target.value)}
            className="w-full rounded border border-slate-300 px-2 py-2 text-sm outline-none focus:border-blue-500"
          >
            {peers.length === 0 && <option value="">{t('collab_no_peers')}</option>}
            {peers.map((peer) => (
              <option key={peer.peerId} value={peer.peerId}>
                {peer.displayName}
              </option>
            ))}
          </select>
          {peerProjects.length > 0 && (
            <div className="space-y-1 text-xs text-slate-500">
              {peerProjects.slice(0, 4).map((project) => (
                <div key={project.projectId} className="flex justify-between gap-2">
                  <span className="truncate">{project.pathLabel}</span>
                  <span className="flex-shrink-0">{project.activeSessionCount}</span>
                </div>
              ))}
            </div>
          )}
          {discoveredPeers.length > 0 && (
            <div className="space-y-1 border-t border-slate-100 pt-2">
              <div className="text-xs font-semibold uppercase text-slate-500">{t('collab_discovered_peers')}</div>
              {discoveredPeers.slice(0, 4).map((peer) => (
                <button
                  type="button"
                  key={peer.peerId}
                  onClick={() => onUseDiscoveredPeer(peer)}
                  className="flex w-full items-center justify-between gap-2 rounded px-2 py-1.5 text-left text-xs text-slate-600 hover:bg-slate-50"
                >
                  <span className="min-w-0 truncate">{peer.displayName}</span>
                  <span className="flex-shrink-0 text-slate-400">{peer.baseUrl.replace(/^https?:\/\//, '')}</span>
                </button>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <select
              value={selectedProject?.projectId ?? ''}
              onChange={(event) => onSelectedProjectIdChange(event.target.value)}
              className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-2 text-sm outline-none focus:border-blue-500"
            >
              {projects.length === 0 && <option value="">{t('collab_no_projects')}</option>}
              {projects.map((project) => (
                <option key={project.projectId} value={project.projectId}>
                  {projectDisplayName(project, duplicateProjectLabels)}
                </option>
              ))}
            </select>
            <input
              type="number"
              min={1}
              max={90}
              value={summaryDays}
              onChange={(event) => onSummaryDaysChange(Number(event.target.value))}
              className="w-16 rounded border border-slate-300 px-2 py-2 text-sm outline-none focus:border-blue-500"
            />
          </div>
          <button
            type="button"
            onClick={onCreateSubscription}
            disabled={!canGenerate}
            className="flex w-full items-center justify-center gap-2 rounded bg-slate-900 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-slate-800 disabled:cursor-not-allowed disabled:bg-slate-300"
          >
            <FileText className="h-4 w-4" />
            {isGenerating ? t('collab_generating') : t('collab_subscribe_baseline')}
          </button>
          {state?.store.subscriptions.length ? (
            <div className="text-xs text-slate-500">
              {t('collab_active_subscriptions', { count: state.store.subscriptions.length })}
            </div>
          ) : null}
          <button
            type="button"
            onClick={onGenerateIncremental}
            disabled={!activeSubscription || isRefreshingIncremental}
            className="flex w-full items-center justify-center gap-2 rounded border border-slate-300 px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400"
          >
            <RefreshCw className={cn('h-4 w-4', isRefreshingIncremental && 'animate-spin')} />
            {isRefreshingIncremental ? t('collab_refreshing_incremental') : t('collab_refresh_incremental')}
          </button>
        </section>

        <section className="space-y-3">
          <div className="text-xs font-semibold uppercase text-slate-500">{t('collab_share_policies')}</div>
          <div className="space-y-2">
            {projects.map((project) => {
              const policy = policies.find((item) => item.projectId === project.projectId);
              const enabled = policy?.enabled ?? false;
              const displayName = projectDisplayName(project, duplicateProjectLabels);
              const detail = projectDetail(project);
              return (
                <div key={project.projectId} className="border-b border-slate-100 pb-2 last:border-b-0">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-slate-800" title={detail}>
                        {displayName}
                      </div>
                      <div className="truncate font-mono text-xs text-slate-500" title={detail}>
                        {detail}
                      </div>
                      <div className="truncate text-xs text-slate-500">{labelsText(policy?.sharedLabels ?? [])}</div>
                    </div>
                    <button
                      type="button"
                      title={enabled ? t('collab_disable_share') : t('collab_enable_share')}
                      disabled={isSavingPolicy}
                      onClick={() => onTogglePolicy(project.projectId, project.rootPath ?? null, !enabled)}
                      className={cn(
                        'mt-0.5 text-slate-400 transition-colors disabled:cursor-not-allowed disabled:opacity-50',
                        enabled && 'text-emerald-600'
                      )}
                    >
                      {enabled ? <ToggleRight className="h-6 w-6" /> : <ToggleLeft className="h-6 w-6" />}
                    </button>
                  </div>
                </div>
              );
            })}
            {projects.length === 0 && <div className="text-sm text-slate-500">{t('collab_no_projects')}</div>}
          </div>
        </section>

        <section className="space-y-3">
          <div className="text-xs font-semibold uppercase text-slate-500">{t('collab_summaries')}</div>
          {activeSummary ? (
            <div className="space-y-2">
              <button
                type="button"
                onClick={() =>
                  setExpandedSummaryId((current) =>
                    current === activeSummary.summaryId ? null : activeSummary.summaryId
                  )
                }
                className="w-full text-left"
              >
                <div className="text-sm font-medium text-slate-800">
                  {new Date(activeSummary.generatedAt).toLocaleString()}
                </div>
                <div className="text-xs text-slate-500">{activeSummary.engine}</div>
              </button>
              <div
                className={cn(
                  'whitespace-pre-wrap rounded border border-slate-200 bg-slate-50 p-3 text-sm leading-6 text-slate-700',
                  expandedSummaryId !== activeSummary.summaryId && 'max-h-64 overflow-hidden'
                )}
              >
                {activeSummary.markdown}
              </div>
            </div>
          ) : (
            <div className="text-sm text-slate-500">{t('collab_no_summary')}</div>
          )}
        </section>
      </div>
    </main>
  );
}
