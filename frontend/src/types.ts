export type SessionStatus = 'active' | 'stale' | 'deleted';
export type SortField = 'lastModified' | 'size';
export type SortDirection = 'asc' | 'desc';

export interface Session {
  id: string;
  codexSessionId?: string | null;
  name: string;
  excerpt: string;
  fullContent?: string;
  path: string;
  projectPath?: string | null;
  labels: string[];
  lastModified: string; // ISO string
  size: number; // bytes
  status: SessionStatus;
  notes?: string;
}

export type BuiltInFilter = 'all' | 'recent' | 'stale' | 'unlabeled' | 'deleted';
export type FilterType = BuiltInFilter | `label:${string}` | `project:${string}`;

export function makeLabelFilter(label: string): FilterType {
  return `label:${label}`;
}

export function makeProjectFilter(projectPath: string): FilterType {
  return `project:${projectPath}`;
}

export function labelFromFilter(filter: FilterType): string | null {
  return filter.startsWith('label:') ? filter.slice('label:'.length) : null;
}

export function projectPathFromFilter(filter: FilterType): string | null {
  return filter.startsWith('project:') ? filter.slice('project:'.length) : null;
}

export interface LabelCount {
  name: string;
  count: number;
}

export interface FilterCounts {
  all: number;
  recent: number;
  stale: number;
  unlabeled: number;
  deleted: number;
}

export interface SessionsResponse {
  workspacePath?: string;
  sessions: Session[];
  counts: FilterCounts;
  labels: LabelCount[];
  staleAfterDays: number;
}

export interface ScanResponse extends SessionsResponse {
  workspacePath: string;
  skippedFiles: number;
}

export type SettingsResponse = SessionsResponse;

export interface ActivitySummaryResponse {
  summary: string;
  days: number;
  sessionCount: number;
  includedSessionCount: number;
  omittedSessionCount: number;
  generatedAt: string;
  activeSince: string;
  engine: string;
}

export interface SessionContentResponse {
  sessionId: string;
  path: string;
  content: string;
  format: 'json' | 'jsonl' | 'text';
  size: number;
  lastModified: string;
}

export interface ProjectIdentity {
  projectId: string;
  rootPath?: string | null;
  pathLabel: string;
  gitRemoteHash?: string | null;
  gitBranch?: string | null;
}

export interface ResolveProjectResponse {
  project: ProjectIdentity;
}

export interface SharePolicy {
  projectId: string;
  projectPath?: string | null;
  enabled: boolean;
  sharedLabels: string[];
  blockedLabels: string[];
  maxExcerptChars: number;
  maxDeltaChars: number;
  updatedAt: string;
}

export interface CollaborationSummary {
  summaryId: string;
  projectId: string;
  sourceIds: string[];
  markdown: string;
  generatedAt: string;
  activeSince: string;
  engine: string;
}

export interface PeerMetadata {
  peerId: string;
  displayName: string;
  trusted: boolean;
  publicKey?: string | null;
  baseUrl?: string | null;
  lastSeenAt?: string | null;
  accessToken?: string | null;
}

export interface PeerProject {
  projectId: string;
  pathLabel: string;
  activeSessionCount: number;
  latestRecordAt?: string | null;
}

export interface PeerPresence {
  peerId: string;
  serviceName: string;
  displayName: string;
  version?: string | null;
  baseUrl: string;
  hostName: string;
  port: number;
  lastSeenAt: string;
}

export interface Subscription {
  subscriptionId: string;
  peerId: string;
  projectId: string;
  status: 'requested' | 'approved' | 'active' | 'paused' | 'revoked';
  topics: string[];
  createdAt: string;
  baselineGeneratedAt?: string | null;
  analysisCycle: '10m' | '1h' | 'manual';
  nextRunAt?: string | null;
  lastRunAt?: string | null;
  lastRunStatus?: string | null;
  lastRunError?: string | null;
}

export interface CollaborationStore {
  schemaVersion: number;
  localPeer?: PeerMetadata | null;
  sources: unknown[];
  trustedPeers: PeerMetadata[];
  projectPolicies: SharePolicy[];
  subscriptions: Subscription[];
  deltaCursors: unknown[];
  summaries: CollaborationSummary[];
  hints: unknown[];
}

export interface LocalCollaborationConfig {
  peerId: string;
  displayName: string;
  baseUrl: string;
  bindAddress: string;
  lanBaseUrls: string[];
  peerToken?: string | null;
  peerTokenConfigured: boolean;
  lanDiscoveryEnabled: boolean;
}

export interface CollaborationStateResponse {
  store: CollaborationStore;
  projects: ProjectIdentity[];
  discoveredPeers: PeerPresence[];
  localConfig: LocalCollaborationConfig;
}

export interface PairPeerResponse {
  state: CollaborationStateResponse;
  peer: PeerMetadata;
  peerProjects: PeerProject[];
}

export interface CreateSubscriptionResponse {
  state: CollaborationStateResponse;
  subscription: Subscription;
  summary: CollaborationSummary;
}

export interface IncrementalSummaryResponse {
  state: CollaborationStateResponse;
  summary: CollaborationSummary;
}

export interface SessionMutationResponse {
  session: Session;
  archiveRecord?: {
    sessionId: string;
    sourcePath: string;
    archiveProvider: string;
    archiveUri: string;
    archivedAt: string;
    checksum?: string;
  };
}
