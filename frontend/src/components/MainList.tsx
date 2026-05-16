import React, { useEffect, useRef } from 'react';
import { ActivitySummaryResponse, Session, SortDirection, SortField } from '../types';
import { formatDistanceToNow } from 'date-fns';
import Markdown from 'react-markdown';
import { cn } from '../lib/utils';
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Folder,
  RefreshCw,
  Save,
  Search,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import { useI18n } from '../i18n';

interface MainListProps {
  sessions: Session[];
  selectedIds: Set<string>;
  focusedId: string | null;
  onToggleSelect: (id: string) => void;
  onSelectAll: () => void;
  onFocus: (id: string) => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  currentFilterText: string;
  workspacePath: string;
  onWorkspacePathChange: (path: string) => void;
  onScan: () => void;
  staleAfterDays: number;
  staleAfterDaysDraft: string;
  onStaleAfterDaysDraftChange: (value: string) => void;
  onSaveStaleAfterDays: () => void;
  isSettingsSaving: boolean;
  isLoading: boolean;
  errorMessage: string | null;
  noticeMessage: string | null;
  activitySummary: ActivitySummaryResponse | null;
  summaryDays: number;
  onSummaryDaysChange: (days: number) => void;
  isGeneratingSummary: boolean;
  onGenerateActivitySummary: () => void;
  onClearActivitySummary: () => void;
  sortField: SortField;
  sortDirection: SortDirection;
  onSortChange: (field: SortField) => void;
  selectedLabelFilters: string[];
  onToggleLabelFilter: (label: string) => void;
  onClearLabelFilters: () => void;
  showBulkActions: boolean;
  selectedCount: number;
  isBulkActionBusy: boolean;
  onBulkArchiveDelete: () => void;
  onClearSelection: () => void;
}

function formatBytes(bytes: number) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function StatusBadge({ status }: { status: string }) {
  const { t } = useI18n();
  if (status === 'stale') {
    return (
      <span className="flex items-center gap-1.5 text-amber-500 text-xs font-semibold">
        <span className="w-1.5 h-1.5 rounded-full bg-amber-500"></span> {t('status_stale')}
      </span>
    );
  }
  if (status === 'deleted') {
    return (
      <span className="flex items-center gap-1.5 text-slate-400 text-xs font-semibold">
        <span className="w-1.5 h-1.5 rounded-full bg-slate-400"></span> {t('status_archived')}
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1.5 text-blue-600 text-xs font-semibold">
      <span className="w-1.5 h-1.5 rounded-full bg-blue-600"></span> {t('status_active')}
    </span>
  );
}

function LabelBadge({
  label,
  index,
  isActive,
  onClick,
}: {
  label: string;
  index: number;
  isActive: boolean;
  onClick: (label: string) => void;
}) {
  const colors = [
    "bg-emerald-100 text-emerald-700",
    "bg-blue-100 text-blue-700",
    "bg-purple-100 text-purple-700",
    "bg-rose-100 text-rose-700",
    "bg-amber-100 text-amber-700"
  ];
  const colorClass = colors[index % colors.length];

  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        onClick(label);
      }}
      className={cn(
        "px-2 py-0.5 rounded text-[10px] font-bold uppercase transition-colors",
        colorClass,
        "hover:ring-1 hover:ring-blue-300 hover:ring-offset-1",
        isActive && "bg-blue-600 text-white ring-1 ring-blue-600 ring-offset-1"
      )}
    >
      {label}
    </button>
  );
}

function SortHeader({
  field,
  label,
  activeField,
  direction,
  onSortChange,
}: {
  field: SortField;
  label: string;
  activeField: SortField;
  direction: SortDirection;
  onSortChange: (field: SortField) => void;
}) {
  const isActive = activeField === field;
  const Icon = isActive ? (direction === 'asc' ? ArrowUp : ArrowDown) : ArrowUpDown;

  return (
    <button
      type="button"
      onClick={() => onSortChange(field)}
      className={cn(
        "inline-flex items-center gap-1.5 transition-colors hover:text-slate-900",
        isActive && "text-blue-600"
      )}
    >
      <span>{label}</span>
      <Icon className="h-3.5 w-3.5" />
    </button>
  );
}

export function MainList({ 
  sessions, 
  selectedIds, 
  focusedId, 
  onToggleSelect, 
  onSelectAll, 
  onFocus,
  searchQuery,
  onSearchChange,
  currentFilterText,
  workspacePath,
  onWorkspacePathChange,
  onScan,
  staleAfterDays,
  staleAfterDaysDraft,
  onStaleAfterDaysDraftChange,
  onSaveStaleAfterDays,
  isSettingsSaving,
  isLoading,
  errorMessage,
  noticeMessage,
  activitySummary,
  summaryDays,
  onSummaryDaysChange,
  isGeneratingSummary,
  onGenerateActivitySummary,
  onClearActivitySummary,
  sortField,
  sortDirection,
  onSortChange,
  selectedLabelFilters,
  onToggleLabelFilter,
  onClearLabelFilters,
  showBulkActions,
  selectedCount,
  isBulkActionBusy,
  onBulkArchiveDelete,
  onClearSelection
}: MainListProps) {
  const { t } = useI18n();
  const selectAllRef = useRef<HTMLInputElement>(null);
  const selectedVisibleCount = sessions.filter(session => selectedIds.has(session.id)).length;
  const allSelected = sessions.length > 0 && selectedVisibleCount === sessions.length;
  const hasPartialSelection = selectedVisibleCount > 0 && !allSelected;
  const activitySummaryGeneratedAt = activitySummary
    ? formatDistanceToNow(new Date(activitySummary.generatedAt), { addSuffix: true })
    : '';
  const summaryDayOptions = [1, 7, 14, 30, 90];

  useEffect(() => {
    if (selectAllRef.current) {
      selectAllRef.current.indeterminate = hasPartialSelection;
    }
  }, [hasPartialSelection]);
  
  return (
    <main className="flex-1 flex min-h-0 flex-col min-w-0 font-sans">
      <header className="min-h-16 bg-white border-b border-slate-200 px-3 py-3 flex items-center gap-3 flex-wrap flex-shrink-0 md:px-6">
        <div className="flex items-center gap-3 flex-wrap min-w-0 flex-1">
          <div className="flex items-center bg-slate-100 rounded-md px-2 py-1 border border-slate-200 focus-within:ring-1 focus-within:ring-blue-400 focus-within:border-blue-400 transition-all text-slate-600 min-w-[220px] max-w-xl flex-[1_1_280px]">
            <Folder className="w-3.5 h-3.5 ml-1 text-slate-400" />
            <input 
              type="text" 
              value={workspacePath}
              onChange={(e) => onWorkspacePathChange(e.target.value)}
              placeholder="/path/to/codex/sessions"
              className="bg-transparent border-none outline-none text-xs font-mono px-2 py-1 min-w-0 flex-1 text-slate-600 placeholder-slate-400"
            />
          </div>
          <button
            onClick={onScan}
            disabled={isLoading}
            className="text-xs bg-white hover:bg-slate-50 disabled:opacity-60 disabled:cursor-not-allowed border border-slate-200 px-3 py-1.5 rounded font-medium shadow-sm flex items-center gap-2 text-slate-700 transition-colors"
          >
            <RefreshCw className={cn("w-3.5 h-3.5 text-slate-400", isLoading && "animate-spin")} />
            {t('btn_scan_path')}
          </button>
          
          <div className="h-4 w-px bg-slate-200 mx-1 hidden xl:block"></div>

          <div className="text-xs bg-white border border-slate-200 px-3 py-1.5 rounded font-medium shadow-sm flex items-center gap-2 focus-within:border-blue-400 focus-within:ring-1 focus-within:ring-blue-400 transition-all min-w-[180px] flex-[0_1_240px]">
            <Search className="w-3.5 h-3.5 text-slate-400" />
            <input 
              type="text" 
              placeholder={t('placeholder_filter')}
              value={searchQuery}
              onChange={(e) => onSearchChange(e.target.value)}
              className="bg-transparent border-none outline-none p-0 text-xs min-w-0 flex-1 text-slate-800 placeholder-slate-400"
            />
          </div>

          <div
            className="flex items-center gap-2 rounded border border-slate-200 bg-white px-2 py-1.5 text-xs font-medium text-slate-600 shadow-sm focus-within:border-amber-400 focus-within:ring-1 focus-within:ring-amber-400"
            title={t('stale_threshold_title', { count: staleAfterDays })}
          >
            <AlertCircle className="h-3.5 w-3.5 shrink-0 text-amber-500" />
            <span className="whitespace-nowrap text-slate-500">{t('stale_threshold_label')}</span>
            <input
              type="number"
              min={1}
              max={3650}
              value={staleAfterDaysDraft}
              onChange={(event) => onStaleAfterDaysDraftChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') onSaveStaleAfterDays();
              }}
              className="w-14 bg-transparent text-center font-mono text-slate-800 outline-none"
            />
            <span className="whitespace-nowrap text-slate-400">{t('stale_threshold_unit')}</span>
            <button
              type="button"
              onClick={onSaveStaleAfterDays}
              disabled={isSettingsSaving}
              className="rounded border border-slate-200 p-1 text-slate-500 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
              title={t('btn_save_stale_threshold')}
              aria-label={t('btn_save_stale_threshold')}
            >
              <Save className={cn("h-3.5 w-3.5", isSettingsSaving && "animate-pulse")} />
            </button>
          </div>
        </div>
        <div className="flex items-center gap-3 shrink-0">
          <div className="inline-flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-xs font-semibold text-slate-600 shadow-sm">
            <span className="whitespace-nowrap text-slate-500">{t('summary_range_label')}</span>
            <select
              value={summaryDays}
              onChange={(event) => onSummaryDaysChange(Number.parseInt(event.target.value, 10))}
              disabled={isGeneratingSummary}
              className="bg-transparent text-xs font-semibold text-slate-800 outline-none disabled:cursor-not-allowed disabled:opacity-60"
              aria-label={t('summary_range_label')}
            >
              {summaryDayOptions.map((days) => (
                <option key={days} value={days}>
                  {t('summary_range_days', { days })}
                </option>
              ))}
            </select>
          </div>
          <button
            type="button"
            onClick={onGenerateActivitySummary}
            disabled={isGeneratingSummary || isLoading}
            className="inline-flex items-center gap-2 rounded-lg border border-blue-200 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-700 shadow-sm transition-colors hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-60"
            title={t('btn_activity_summary')}
          >
            <Sparkles className={cn("h-4 w-4", isGeneratingSummary && "animate-pulse")} />
            <span>{isGeneratingSummary ? t('btn_summary_processing') : t('btn_activity_summary')}</span>
          </button>
          <button className="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-lg text-sm font-medium transition-colors shadow-sm max-w-[280px] whitespace-nowrap">
            <span className="block truncate">{currentFilterText} ({sessions.length})</span>
          </button>
        </div>
      </header>

      <div className="flex-1 min-h-0 overflow-hidden p-3 bg-[#F8FAFC] md:p-6">
        {errorMessage && (
          <div className="mb-3 border border-red-200 bg-red-50 text-red-700 rounded-lg px-4 py-3 text-sm font-medium">
            {errorMessage}
          </div>
        )}
        {noticeMessage && (
          <div className="mb-3 border border-amber-200 bg-amber-50 text-amber-700 rounded-lg px-4 py-3 text-sm font-medium">
            {noticeMessage}
          </div>
        )}
        {activitySummary && (
          <section className="mb-3 overflow-hidden rounded-xl border border-blue-200 bg-white shadow-sm">
            <div className="flex flex-col gap-2 border-b border-blue-100 bg-blue-50 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="flex items-center gap-2 text-sm font-bold text-blue-950">
                  <Sparkles className="h-4 w-4 shrink-0 text-blue-600" />
                  <span>{t('activity_summary_title')}</span>
                </div>
                <p className="mt-1 text-xs text-blue-700">
                  {t('activity_summary_meta', {
                    count: activitySummary.sessionCount,
                    days: activitySummary.days,
                    included: activitySummary.includedSessionCount,
                    omitted: activitySummary.omittedSessionCount,
                    time: activitySummaryGeneratedAt,
                    engine: activitySummary.engine,
                  })}
                </p>
              </div>
              <button
                type="button"
                onClick={onClearActivitySummary}
                className="self-start rounded border border-blue-200 bg-white p-1.5 text-blue-600 transition-colors hover:bg-blue-50 sm:self-auto"
                title={t('btn_close_summary')}
                aria-label={t('btn_close_summary')}
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="max-h-72 overflow-y-auto px-4 py-4 text-sm leading-6 text-slate-700">
              <div className="prose prose-sm max-w-none prose-headings:text-slate-900 prose-li:my-0 prose-p:my-2">
                <Markdown>{activitySummary.summary}</Markdown>
              </div>
            </div>
          </section>
        )}
        {selectedLabelFilters.length > 0 && (
          <div className="mb-3 flex flex-col gap-2 rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 shadow-sm sm:flex-row sm:items-center sm:justify-between">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                {t('active_label_filters')}
              </span>
              {selectedLabelFilters.map((label) => (
                <button
                  key={label}
                  type="button"
                  onClick={() => onToggleLabelFilter(label)}
                  className="inline-flex max-w-[220px] items-center gap-1 rounded bg-blue-600 px-2 py-1 text-xs font-semibold text-white transition-colors hover:bg-blue-700"
                  title={label}
                >
                  <span className="truncate">{label}</span>
                  <X className="h-3 w-3 shrink-0" />
                </button>
              ))}
              <span className="text-xs text-slate-400">{t('label_filter_and_hint')}</span>
            </div>
            <button
              type="button"
              onClick={onClearLabelFilters}
              className="self-start rounded border border-slate-300 bg-white px-2.5 py-1 text-xs font-semibold text-slate-600 transition-colors hover:bg-slate-50 sm:self-auto"
            >
              {t('btn_clear_filters')}
            </button>
          </div>
        )}
        {showBulkActions && (
          <div className="mb-3 flex flex-col gap-3 rounded-lg border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-slate-700 shadow-sm sm:flex-row sm:items-center sm:justify-between">
            <div className="font-medium text-blue-900">
              {t('bulk_selected_count', { count: selectedCount })}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={onBulkArchiveDelete}
                disabled={isBulkActionBusy}
                className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <Trash2 className="h-3.5 w-3.5" />
                {isBulkActionBusy ? t('btn_bulk_processing') : t('btn_bulk_move_recycle')}
              </button>
              <button
                type="button"
                onClick={onClearSelection}
                disabled={isBulkActionBusy}
                className="inline-flex items-center gap-2 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-semibold text-slate-600 shadow-sm transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <X className="h-3.5 w-3.5" />
                {t('btn_clear_selection')}
              </button>
            </div>
          </div>
        )}
        <div className="bg-white border border-slate-200 rounded-xl shadow-sm h-full flex flex-col overflow-hidden">
          <div className="flex-1 min-h-0 overflow-auto">
            <table className="w-full min-w-[980px] text-left">
              <thead className="bg-slate-50 border-b border-slate-200 text-xs font-semibold text-slate-500 uppercase tracking-wider sticky top-0 z-10 shadow-sm">
                <tr>
                  <th className="px-4 py-3 w-10 text-center">
                    <input
                      ref={selectAllRef}
                      type="checkbox"
                      checked={allSelected}
                      onChange={onSelectAll}
                      disabled={isBulkActionBusy}
                      aria-label={t('select_all_visible')}
                      className="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-60"
                    />
                  </th>
                  <th className="px-4 py-3">{t('table_name')}</th>
                  <th className="px-4 py-3">{t('table_project_path')}</th>
                  <th className="px-4 py-3">{t('table_labels')}</th>
                  <th className="px-4 py-3">
                    <SortHeader
                      field="lastModified"
                      label={t('table_modified')}
                      activeField={sortField}
                      direction={sortDirection}
                      onSortChange={onSortChange}
                    />
                  </th>
                  <th className="px-4 py-3">
                    <SortHeader
                      field="size"
                      label={t('table_size')}
                      activeField={sortField}
                      direction={sortDirection}
                      onSortChange={onSortChange}
                    />
                  </th>
                  <th className="px-4 py-3">{t('table_status')}</th>
                </tr>
              </thead>
              <tbody className="text-sm divide-y divide-slate-100">
                {sessions.map((session) => {
                  const isSelected = selectedIds.has(session.id);
                  const isFocused = focusedId === session.id;
                  
                  return (
                    <tr 
                      key={session.id} 
                      onClick={() => onFocus(session.id)}
                      className={cn(
                        "cursor-pointer group transition-colors",
                        isFocused ? "bg-blue-50/50" : (isSelected ? "bg-blue-50/30" : "hover:bg-slate-50")
                      )}
                    >
                      <td
                        className="px-4 py-3 text-center"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (!isBulkActionBusy) onToggleSelect(session.id);
                        }}
                      >
                        <input 
                          type="checkbox" 
                          checked={isSelected} 
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => {
                            e.stopPropagation();
                            if (!isBulkActionBusy) onToggleSelect(session.id);
                          }}
                          disabled={isBulkActionBusy}
                          className="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-60"
                        />
                      </td>
                      <td className="px-4 py-3 min-w-[250px] max-w-[300px]">
                        <div className="font-medium text-slate-900 truncate">{session.name}</div>
                        <div className="text-xs text-slate-500 truncate w-full mt-0.5">{session.excerpt}</div>
                      </td>
                      <td className="px-4 py-3 min-w-[220px] max-w-[280px]">
                        {session.projectPath ? (
                          <div className="font-mono text-xs text-slate-600 truncate" title={session.projectPath}>
                            {session.projectPath}
                          </div>
                        ) : (
                          <span className="text-slate-400 text-xs italic">{t('project_path_unknown')}</span>
                        )}
                      </td>
                      <td className="px-4 py-3">
                        <div className="flex flex-wrap gap-1.5">
                          {session.labels.map((label, idx) => (
                            <LabelBadge
                              key={label}
                              label={label}
                              index={idx}
                              isActive={selectedLabelFilters.includes(label)}
                              onClick={onToggleLabelFilter}
                            />
                          ))}
                          {session.labels.length === 0 && <span className="text-slate-400 text-[10px] italic">{t('no_labels')}</span>}
                        </div>
                      </td>
                      <td className="px-4 py-3 text-slate-500 text-xs whitespace-nowrap">
                        {formatDistanceToNow(new Date(session.lastModified), { addSuffix: true })}
                      </td>
                      <td className="px-4 py-3 text-slate-500 text-xs whitespace-nowrap">
                        {formatBytes(session.size)}
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">
                        <StatusBadge status={session.status} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            
            {sessions.length === 0 && (
              <div className="py-24 text-center flex flex-col items-center justify-center text-slate-400">
                <Search className="w-10 h-10 mb-3 opacity-20" />
                <p className="text-base text-slate-600 font-medium tracking-tight">{t('no_sessions')}</p>
              </div>
            )}
          </div>
          
          <div className="min-h-12 border-t border-slate-100 p-3 flex flex-col gap-3 text-xs text-slate-500 font-medium bg-white shrink-0 mt-auto sm:flex-row sm:items-center sm:justify-between md:p-4">
            <div>{t('showing_sessions', { count: sessions.length })}</div>
            <div className="flex items-center gap-2">
              <button className="px-2 py-1 border rounded text-slate-400 cursor-not-allowed">{t('btn_prev')}</button>
              <button className="px-2 py-1 border border-slate-300 rounded bg-slate-50 text-slate-700">1</button>
              <button className="px-2 py-1 border rounded text-slate-400 cursor-not-allowed">{t('btn_next')}</button>
            </div>
          </div>
        </div>
      </div>
    </main>
  );
}
