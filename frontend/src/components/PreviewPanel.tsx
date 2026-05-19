import React, { useEffect, useMemo, useState } from 'react';
import { Session, SessionContentResponse, SessionMutationResponse } from '../types';
import Markdown from 'react-markdown';
import {
  AlertCircle,
  Archive,
  Check,
  ClipboardCheck,
  Code2,
  Copy,
  FileText,
  Loader2,
  LockKeyhole,
  Pencil,
  RotateCcw,
  Search,
  ShieldCheck,
  Terminal,
  Trash2,
  X,
} from 'lucide-react';
import { cn } from '../lib/utils';
import { useI18n } from '../i18n';

interface PreviewPanelProps {
  session: Session | null;
  onClose: () => void;
  onDelete: (id: string) => Promise<void>;
  onRestore: (id: string) => Promise<void>;
  onUpdateLabels: (id: string, labels: string[]) => Promise<void>;
  onUpdateNotes: (id: string, notes: string) => Promise<void>;
  onLoadContent: (id: string) => Promise<SessionContentResponse>;
  onUpdateContent: (id: string, content: string) => Promise<SessionMutationResponse>;
  isBusy: boolean;
}

type CopyStatus = 'idle' | 'copied' | 'failed';
type PreviewMode = 'view' | 'edit' | 'review';
type ContentEditorMode = 'edit' | 'review';
const EMPTY_LABELS: string[] = [];
const REVIEW_PREVIEW_CHAR_LIMIT = 24_000;

interface ContentValidationResult {
  valid: boolean;
  message: string;
  line?: number;
}

interface JsonlRecordDraft {
  lineNumber: number;
  originalDraft: string;
  draft: string;
  title: string;
  subtitle: string;
  type: string;
}

function normalizeLabels(value: string) {
  const seen = new Set<string>();
  const labels: string[] = [];

  value
    .split(/[,\n]/)
    .map((label) => label.trim())
    .filter(Boolean)
    .forEach((label) => {
      if (seen.has(label)) return;
      seen.add(label);
      labels.push(label);
    });

  return labels;
}

function labelsEqual(left: string[], right: string[]) {
  if (left.length !== right.length) return false;
  return left.every((label, index) => label === right[index]);
}

function previewValue(value: string) {
  return value.trim() || '—';
}

function validateRawContent(
  format: SessionContentResponse['format'] | 'unknown',
  content: string,
  t: (key: string, params?: Record<string, string | number>) => string
): ContentValidationResult {
  if (format === 'json') {
    try {
      JSON.parse(content);
      return { valid: true, message: t('content_validation_json_ok') };
    } catch (error) {
      return {
        valid: false,
        message: error instanceof Error ? error.message : t('content_validation_json_failed')
      };
    }
  }

  if (format === 'jsonl') {
    const lines = content.split(/\r?\n/);
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index].trim();
      if (!line) continue;
      try {
        JSON.parse(line);
      } catch (error) {
        return {
          valid: false,
          line: index + 1,
          message: error instanceof Error
            ? t('content_validation_jsonl_failed_with_message', { line: index + 1, message: error.message })
            : t('content_validation_jsonl_failed', { line: index + 1 })
        };
      }
    }
    return { valid: true, message: t('content_validation_jsonl_ok', { count: lines.length }) };
  }

  return { valid: true, message: t('content_validation_text_ok') };
}

function describeJsonlValue(value: unknown, index: number) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return {
      title: `#${index + 1}`,
      subtitle: typeof value,
      type: typeof value,
    };
  }

  const objectValue = value as Record<string, unknown>;
  const payload = objectValue.payload && typeof objectValue.payload === 'object' && !Array.isArray(objectValue.payload)
    ? objectValue.payload as Record<string, unknown>
    : null;
  const type = [objectValue.type, payload?.type, objectValue.name]
    .find((candidate): candidate is string => typeof candidate === 'string' && candidate.trim().length > 0)
    ?.trim() || 'record';
  const timestamp = typeof objectValue.timestamp === 'string' ? objectValue.timestamp : '';
  const callName = typeof payload?.name === 'string' ? payload.name : '';
  const role = typeof objectValue.role === 'string' ? objectValue.role : '';
  const titleDetail = callName || role || timestamp;

  return {
    title: titleDetail ? `${type} · ${titleDetail}` : type,
    subtitle: timestamp || callName || role || tFallbackObjectKeys(objectValue),
    type,
  };
}

function tFallbackObjectKeys(value: Record<string, unknown>) {
  return Object.keys(value).slice(0, 4).join(', ') || 'object';
}

function parseJsonlRecordDrafts(content: string): JsonlRecordDraft[] {
  return content
    .split(/\r?\n/)
    .map((line, lineIndex) => ({ line, lineNumber: lineIndex + 1 }))
    .filter(({ line }) => line.trim().length > 0)
    .map(({ line, lineNumber }, index) => {
      const parsed = JSON.parse(line);
      const pretty = JSON.stringify(parsed, null, 2);
      const descriptor = describeJsonlValue(parsed, index);

      return {
        lineNumber,
        originalDraft: pretty,
        draft: pretty,
        title: descriptor.title,
        subtitle: descriptor.subtitle,
        type: descriptor.type,
      };
    });
}

function serializeJsonlRecordDrafts(
  records: JsonlRecordDraft[],
  t: (key: string, params?: Record<string, string | number>) => string
) {
  const lines: string[] = [];

  for (let index = 0; index < records.length; index += 1) {
    const record = records[index];
    try {
      lines.push(JSON.stringify(JSON.parse(record.draft)));
    } catch (error) {
      return {
        content: records.map((item) => item.draft).join('\n'),
        validation: {
          valid: false,
          line: record.lineNumber,
          message: error instanceof Error
            ? t('content_validation_jsonl_record_failed_with_message', {
              record: index + 1,
              line: record.lineNumber,
              message: error.message,
            })
            : t('content_validation_jsonl_failed', { line: record.lineNumber }),
        } satisfies ContentValidationResult,
      };
    }
  }

  return {
    content: lines.join('\n'),
    validation: {
      valid: true,
      message: t('content_validation_jsonl_ok', { count: records.length }),
    } satisfies ContentValidationResult,
  };
}

function normalizeJsonlForComparison(content: string) {
  try {
    return serializeJsonlRecordDrafts(parseJsonlRecordDrafts(content), (_, params) => String(params?.count || '')).content;
  } catch {
    return content;
  }
}

function contentStats(content: string) {
  return {
    chars: content.length,
    lines: content.length === 0 ? 0 : content.split(/\r?\n/).length,
  };
}

function reviewPreview(
  content: string,
  t: (key: string, params?: Record<string, string | number>) => string
) {
  if (content.length <= REVIEW_PREVIEW_CHAR_LIMIT) {
    return content;
  }

  const edgeLength = Math.floor(REVIEW_PREVIEW_CHAR_LIMIT / 2);
  const omitted = content.length - edgeLength * 2;
  return [
    content.slice(0, edgeLength),
    t('content_editor_omitted_preview', { chars: omitted }),
    content.slice(content.length - edgeLength),
  ].join('\n\n');
}

async function copyText(text: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall through to the textarea fallback below.
  }

  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.top = '0';
  textarea.style.left = '0';
  textarea.style.width = '1px';
  textarea.style.height = '1px';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, text.length);

  try {
    return document.execCommand('copy');
  } catch {
    return false;
  } finally {
    document.body.removeChild(textarea);
  }
}

export function PreviewPanel({
  session,
  onClose,
  onDelete,
  onRestore,
  onUpdateLabels,
  onUpdateNotes,
  onLoadContent,
  onUpdateContent,
  isBusy
}: PreviewPanelProps) {
  const { t } = useI18n();
  const [mode, setMode] = useState<PreviewMode>('view');
  const [labelsDraft, setLabelsDraft] = useState('');
  const [notesDraft, setNotesDraft] = useState('');
  const [isSavingEdit, setIsSavingEdit] = useState(false);
  const [contentEditorOpen, setContentEditorOpen] = useState(false);
  const [contentEditorMode, setContentEditorMode] = useState<ContentEditorMode>('edit');
  const [contentSnapshot, setContentSnapshot] = useState<SessionContentResponse | null>(null);
  const [contentDraft, setContentDraft] = useState('');
  const [jsonlRecordEditorEnabled, setJsonlRecordEditorEnabled] = useState(false);
  const [jsonlRecords, setJsonlRecords] = useState<JsonlRecordDraft[]>([]);
  const [selectedJsonlRecordIndex, setSelectedJsonlRecordIndex] = useState(0);
  const [jsonlSearchQuery, setJsonlSearchQuery] = useState('');
  const [isLoadingContent, setIsLoadingContent] = useState(false);
  const [isSavingContent, setIsSavingContent] = useState(false);
  const [contentError, setContentError] = useState<string | null>(null);
  const [pathCopyStatus, setPathCopyStatus] = useState<CopyStatus>('idle');
  const [resumeCopyStatus, setResumeCopyStatus] = useState<CopyStatus>('idle');

  useEffect(() => {
    setMode('view');
    setLabelsDraft((session?.labels || []).join(', '));
    setNotesDraft(session?.notes || '');
    setIsSavingEdit(false);
    setContentEditorOpen(false);
    setContentEditorMode('edit');
    setContentSnapshot(null);
    setContentDraft('');
    setJsonlRecordEditorEnabled(false);
    setJsonlRecords([]);
    setSelectedJsonlRecordIndex(0);
    setJsonlSearchQuery('');
    setIsLoadingContent(false);
    setIsSavingContent(false);
    setContentError(null);
    setPathCopyStatus('idle');
    setResumeCopyStatus('idle');
  }, [session?.id, session?.labels, session?.notes]);

  useEffect(() => {
    if (pathCopyStatus === 'idle') return;

    const timeout = window.setTimeout(() => setPathCopyStatus('idle'), 1800);
    return () => window.clearTimeout(timeout);
  }, [pathCopyStatus]);

  useEffect(() => {
    if (resumeCopyStatus === 'idle') return;

    const timeout = window.setTimeout(() => setResumeCopyStatus('idle'), 1800);
    return () => window.clearTimeout(timeout);
  }, [resumeCopyStatus]);

  const currentLabels = session?.labels ?? EMPTY_LABELS;
  const currentNotes = session?.notes || '';
  const normalizedLabelsDraft = useMemo(() => normalizeLabels(labelsDraft), [labelsDraft]);
  const labelsChanged = useMemo(
    () => !labelsEqual(normalizedLabelsDraft, currentLabels),
    [normalizedLabelsDraft, currentLabels]
  );
  const notesChanged = notesDraft !== currentNotes;
  const hasChanges = labelsChanged || notesChanged;
  const validationErrors = useMemo(() => {
    const errors: string[] = [];
    const rawLabels = labelsDraft
      .split(/[,\n]/)
      .map((label) => label.trim())
      .filter(Boolean);

    if (rawLabels.length > 20) {
      errors.push(t('edit_error_too_many_labels'));
    }
    if (rawLabels.some((label) => label.length > 48)) {
      errors.push(t('edit_error_label_too_long'));
    }
    if (rawLabels.some((label) => /[\u0000-\u001f]/.test(label))) {
      errors.push(t('edit_error_label_control_chars'));
    }
    if (notesDraft.length > 4000) {
      errors.push(t('edit_error_notes_too_long'));
    }

    return errors;
  }, [labelsDraft, notesDraft, t]);
  const canReview = hasChanges && validationErrors.length === 0;
  const contentFormat = contentSnapshot?.format || 'unknown';
  const isJsonlRecordEditor = contentFormat === 'jsonl' && jsonlRecordEditorEnabled;
  const normalizedJsonlSearchQuery = jsonlSearchQuery.trim().toLowerCase();
  const filteredJsonlRecordEntries = useMemo(
    () => jsonlRecords
      .map((record, index) => ({ record, index }))
      .filter(({ record }) => {
        if (!normalizedJsonlSearchQuery) return true;

        return [
          record.title,
          record.subtitle,
          record.type,
          String(record.lineNumber),
          record.draft,
        ].some((value) => value.toLowerCase().includes(normalizedJsonlSearchQuery));
      }),
    [jsonlRecords, normalizedJsonlSearchQuery]
  );
  const jsonlDraftSerialization = useMemo(
    () => isJsonlRecordEditor ? serializeJsonlRecordDrafts(jsonlRecords, t) : null,
    [isJsonlRecordEditor, jsonlRecords, t]
  );
  const contentValidation = useMemo(
    () => jsonlDraftSerialization?.validation || validateRawContent(contentFormat, contentDraft, t),
    [contentDraft, contentFormat, jsonlDraftSerialization, t]
  );
  const contentOriginal = contentSnapshot?.content || '';
  const contentOriginalForComparison = useMemo(
    () => isJsonlRecordEditor ? normalizeJsonlForComparison(contentOriginal) : contentOriginal,
    [contentOriginal, isJsonlRecordEditor]
  );
  const contentDraftForDisplay = jsonlDraftSerialization?.content ?? contentDraft;
  const contentChanged = contentSnapshot !== null && (
    isJsonlRecordEditor
      ? contentDraftForDisplay !== contentOriginalForComparison
      : contentDraft !== contentOriginal
  );
  const contentOriginalStats = useMemo(() => contentStats(contentOriginal), [contentOriginal]);
  const contentDraftStats = useMemo(() => contentStats(contentDraftForDisplay), [contentDraftForDisplay]);
  const contentOriginalReview = useMemo(() => reviewPreview(contentOriginal, t), [contentOriginal, t]);
  const contentDraftReview = useMemo(() => reviewPreview(contentDraftForDisplay, t), [contentDraftForDisplay, t]);
  const canReviewContent = contentChanged && contentValidation.valid && !isLoadingContent;
  const isEditBusy = isBusy || isSavingEdit || isSavingContent;

  useEffect(() => {
    if (!isJsonlRecordEditor || filteredJsonlRecordEntries.length === 0) return;
    if (filteredJsonlRecordEntries.some(({ index }) => index === selectedJsonlRecordIndex)) return;
    setSelectedJsonlRecordIndex(filteredJsonlRecordEntries[0].index);
  }, [filteredJsonlRecordEntries, isJsonlRecordEditor, selectedJsonlRecordIndex]);

  if (!session) {
    return (
      <aside className="w-80 bg-white border-l border-slate-200 hidden lg:flex flex-col items-center justify-center text-slate-400 p-8 text-center font-sans shadow-2xl z-20">
        <FileText className="w-12 h-12 mb-4 opacity-20" />
        <h3 className="text-sm font-medium text-slate-600">{t('panel_empty_title')}</h3>
        <p className="text-xs mt-2">{t('panel_empty_desc')}</p>
      </aside>
    );
  }

  const isDeleted = session.status === 'deleted';
  const selectedJsonlRecord = jsonlRecords[selectedJsonlRecordIndex] || null;

  const resetDrafts = () => {
    setLabelsDraft(session.labels.join(', '));
    setNotesDraft(session.notes || '');
  };

  const startEdit = () => {
    resetDrafts();
    setMode('edit');
  };

  const cancelEdit = () => {
    resetDrafts();
    setMode('view');
  };

  const reviewChanges = () => {
    if (!canReview) return;
    setMode('review');
  };

  const applyChanges = async () => {
    if (!canReview) return;

    setIsSavingEdit(true);
    try {
      if (labelsChanged) {
        await onUpdateLabels(session.id, normalizedLabelsDraft);
      }
      if (notesChanged) {
        await onUpdateNotes(session.id, notesDraft);
      }
      setMode('view');
    } finally {
      setIsSavingEdit(false);
    }
  };

  const openContentEditor = async () => {
    if (!session || isEditBusy) return;

    setContentEditorOpen(true);
    setContentEditorMode('edit');
    setContentSnapshot(null);
    setContentDraft('');
    setJsonlRecordEditorEnabled(false);
    setContentError(null);
    setIsLoadingContent(true);
    try {
      const response = await onLoadContent(session.id);
      let records: JsonlRecordDraft[] = [];
      let useJsonlRecordEditor = false;
      if (response.format === 'jsonl') {
        try {
          records = parseJsonlRecordDrafts(response.content);
          useJsonlRecordEditor = true;
        } catch {
          records = [];
        }
      }
      setContentSnapshot(response);
      setJsonlRecordEditorEnabled(useJsonlRecordEditor);
      setJsonlRecords(records);
      setSelectedJsonlRecordIndex(0);
      setJsonlSearchQuery('');
      setContentDraft(response.format === 'jsonl' ? normalizeJsonlForComparison(response.content) : response.content);
    } catch (error) {
      setContentError(error instanceof Error ? error.message : t('error_load_session_content_failed'));
    } finally {
      setIsLoadingContent(false);
    }
  };

  const closeContentEditor = () => {
    if (isSavingContent) return;
    setContentEditorOpen(false);
    setContentEditorMode('edit');
    setContentError(null);
  };

  const updateJsonlRecordDraft = (value: string) => {
    setJsonlRecords((records) => records.map((record, index) => (
      index === selectedJsonlRecordIndex ? { ...record, draft: value } : record
    )));
  };

  const revertJsonlRecordDraft = () => {
    setJsonlRecords((records) => records.map((record, index) => (
      index === selectedJsonlRecordIndex ? { ...record, draft: record.originalDraft } : record
    )));
  };

  const deleteJsonlRecord = (indexToDelete: number) => {
    if (isSavingContent) return;

    setJsonlRecords((records) => {
      const nextRecords = records.filter((_, index) => index !== indexToDelete);
      setSelectedJsonlRecordIndex((currentIndex) => {
        if (nextRecords.length === 0) return 0;
        if (currentIndex > indexToDelete) return currentIndex - 1;
        if (currentIndex === indexToDelete) {
          return Math.min(indexToDelete, nextRecords.length - 1);
        }
        return currentIndex;
      });
      return nextRecords;
    });
  };

  const reviewContentChanges = () => {
    if (!canReviewContent) return;
    setContentEditorMode('review');
  };

  const applyContentChanges = async () => {
    if (!session || !canReviewContent) return;

    setIsSavingContent(true);
    setContentError(null);
    try {
      await onUpdateContent(session.id, contentDraftForDisplay);
      closeContentEditor();
    } catch (error) {
      setContentError(error instanceof Error ? error.message : t('error_save_session_content_failed'));
      setContentEditorMode('edit');
    } finally {
      setIsSavingContent(false);
    }
  };

  const copyPath = async () => {
    const copied = await copyText(session.path);
    setPathCopyStatus(copied ? 'copied' : 'failed');
  };

  const resumeCommand = session.codexSessionId
    ? `codex resume ${session.codexSessionId}`
    : null;

  const copyResumeCommand = async () => {
    if (!resumeCommand) return;

    const copied = await copyText(resumeCommand);
    setResumeCopyStatus(copied ? 'copied' : 'failed');
  };

  const PathCopyIcon = pathCopyStatus === 'copied' ? Check : Copy;
  const ResumeCopyIcon = resumeCopyStatus === 'copied' ? Check : Copy;
  const copyButtonText = pathCopyStatus === 'copied'
    ? t('btn_copy_path_copied')
    : pathCopyStatus === 'failed'
      ? t('btn_copy_path_failed')
      : t('btn_copy_path');
  const resumeCopyTitle = resumeCopyStatus === 'copied'
    ? t('btn_copy_resume_copied')
    : resumeCopyStatus === 'failed'
      ? t('btn_copy_resume_failed')
      : t('btn_copy_resume');

  return (
    <aside className="fixed inset-0 w-full bg-white border-l border-slate-200 flex flex-col overflow-hidden shadow-2xl z-30 transition-all duration-300 font-sans md:relative md:inset-auto md:w-80 md:flex-shrink-0 md:z-20">
      <div className="p-6 border-b border-slate-100 bg-slate-50/50 shrink-0">
        <div className="flex justify-between items-start mb-4">
          <div className="min-w-0">
            <h2 className="font-bold text-slate-900 leading-tight">{t('panel_preview')}</h2>
            {mode !== 'view' && (
              <div className="mt-1 inline-flex items-center gap-1.5 rounded border border-blue-100 bg-blue-50 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-blue-700">
                <ShieldCheck className="h-3 w-3" />
                {mode === 'review' ? t('edit_review_mode') : t('edit_active_mode')}
              </div>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            {mode === 'view' && (
              <button
                type="button"
                onClick={startEdit}
                disabled={isBusy}
                title={t('btn_edit_session')}
                aria-label={t('btn_edit_session')}
                className="rounded-md border border-slate-200 bg-white p-1.5 text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-800 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <Pencil className="h-4 w-4" />
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded-md p-1.5 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
              title={t('collab_close')}
              aria-label={t('collab_close')}
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>
        <div className="text-xl font-bold text-slate-900 overflow-hidden text-ellipsis truncate" title={session.name}>
          {session.name}
        </div>
        <p className="text-xs text-slate-500 mt-1 uppercase font-semibold tracking-wide truncate" title={session.path}>
          {t('panel_path')} {session.path}
        </p>
        {resumeCommand && (
          <div className="mt-3 rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs text-slate-700">
            <div className="mb-1 flex items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-1.5 font-semibold uppercase tracking-wide text-slate-400">
                <Terminal className="h-3.5 w-3.5 shrink-0" />
                <span>{t('panel_resume_command')}</span>
              </div>
              <button
                type="button"
                onClick={copyResumeCommand}
                title={resumeCopyTitle}
                aria-label={resumeCopyTitle}
                className={cn(
                  "shrink-0 rounded border px-1.5 py-1 transition-colors",
                  resumeCopyStatus === 'copied' && "border-emerald-200 text-emerald-600",
                  resumeCopyStatus === 'failed' && "border-red-200 text-red-600",
                  resumeCopyStatus === 'idle' && "border-slate-200 text-slate-500 hover:bg-slate-50"
                )}
              >
                <ResumeCopyIcon className="h-3.5 w-3.5" />
              </button>
            </div>
            <code className="block truncate font-mono text-[11px] text-slate-900" title={resumeCommand}>
              {resumeCommand}
            </code>
          </div>
        )}
      </div>

      <div className="flex-1 p-6 overflow-y-auto space-y-6">
        <div>
          <div className="mb-2 flex items-center justify-between gap-3">
            <label className="text-[10px] font-bold text-slate-400 uppercase">{t('panel_content')}</label>
            <button
              type="button"
              onClick={openContentEditor}
              disabled={isEditBusy || mode !== 'view'}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-2 py-1 text-[11px] font-semibold text-slate-600 transition-colors hover:bg-slate-50 hover:text-slate-900 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Code2 className="h-3.5 w-3.5" />
              {t('btn_edit_raw_content')}
            </button>
          </div>
          <div className="p-4 bg-slate-50 rounded-lg border border-slate-100 text-xs leading-relaxed text-slate-700 font-mono max-h-44 overflow-y-auto">
            <div className="prose prose-sm prose-slate max-w-none 
                          prose-headings:text-slate-800 prose-headings:font-bold 
                          prose-p:text-slate-600 prose-p:leading-relaxed 
                          prose-a:text-blue-500 
                          prose-code:text-rose-500 prose-code:bg-rose-50 prose-code:px-1 prose-code:rounded
                          prose-pre:bg-slate-900 prose-pre:text-slate-200">
              <Markdown>{session.fullContent || session.excerpt}</Markdown>
            </div>
          </div>
        </div>

        <div>
          <label className="text-[10px] font-bold text-slate-400 uppercase mb-2 block">{t('table_labels')}</label>
          {mode === 'view' ? (
            <div className="flex flex-wrap gap-2">
              {session.labels.map((label, idx) => {
                const colors = [
                  "bg-emerald-50 text-emerald-700 border-emerald-200",
                  "bg-blue-50 text-blue-700 border-blue-200",
                  "bg-purple-50 text-purple-700 border-purple-200",
                  "bg-rose-50 text-rose-700 border-rose-200",
                  "bg-amber-50 text-amber-700 border-amber-200"
                ];
                const colorClass = colors[idx % colors.length];

                return (
                  <span key={label} className={cn("px-2 py-1 border rounded text-[10px] font-bold uppercase", colorClass)}>
                    {label}
                  </span>
                );
              })}
              {session.labels.length === 0 && (
                <span className="text-xs italic text-slate-400">{t('no_labels')}</span>
              )}
            </div>
          ) : (
            <div className="space-y-2">
              <input
                value={labelsDraft}
                onChange={(event) => setLabelsDraft(event.target.value)}
                disabled={isEditBusy || mode === 'review'}
                className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs text-slate-800 outline-none transition-shadow focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:bg-slate-50 disabled:text-slate-500"
                placeholder={t('edit_labels_placeholder')}
              />
              <p className="text-[11px] leading-relaxed text-slate-400">{t('edit_labels_helper')}</p>
              {normalizedLabelsDraft.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {normalizedLabelsDraft.map((label) => (
                    <span key={label} className="rounded border border-blue-100 bg-blue-50 px-2 py-0.5 text-[10px] font-bold uppercase text-blue-700">
                      {label}
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <div>
          <label className="text-[10px] font-bold text-slate-400 uppercase mb-2 block">{t('panel_notes')}</label>
          {mode === 'view' ? (
            <div className="min-h-20 rounded-lg border border-slate-100 bg-slate-50 p-3 text-xs leading-relaxed text-slate-600">
              {session.notes?.trim() ? session.notes : t('panel_notes_empty')}
            </div>
          ) : (
            <textarea
              value={notesDraft}
              onChange={(event) => setNotesDraft(event.target.value)}
              disabled={isEditBusy || mode === 'review'}
              className="h-28 w-full resize-none rounded-lg border border-slate-200 bg-white p-3 text-xs outline-none transition-shadow focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:bg-slate-50 disabled:text-slate-500"
              placeholder={t('panel_notes_placeholder')}
            ></textarea>
          )}
        </div>

        {mode !== 'view' && (
          <div className="space-y-3 rounded-lg border border-slate-200 bg-white p-3">
            <div className="flex items-start gap-2 text-xs leading-relaxed text-slate-600">
              <LockKeyhole className="mt-0.5 h-3.5 w-3.5 shrink-0 text-slate-400" />
              <span>{t('edit_locked_fields_hint')}</span>
            </div>
            {validationErrors.length > 0 && (
              <div className="space-y-1 rounded-md border border-red-100 bg-red-50 p-2 text-xs font-medium text-red-700">
                {validationErrors.map((error) => (
                  <div key={error} className="flex items-start gap-1.5">
                    <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span>{error}</span>
                  </div>
                ))}
              </div>
            )}
            {mode === 'review' && (
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-slate-500">
                  <ClipboardCheck className="h-3.5 w-3.5 text-blue-500" />
                  {t('edit_review_changes')}
                </div>
                {labelsChanged && (
                  <div className="rounded-md border border-slate-100 bg-slate-50 p-2 text-xs">
                    <div className="mb-1 font-semibold text-slate-700">{t('table_labels')}</div>
                    <div className="grid gap-1 text-[11px]">
                      <div><span className="font-semibold text-slate-400">{t('edit_before')}</span> {previewValue(session.labels.join(', '))}</div>
                      <div><span className="font-semibold text-blue-500">{t('edit_after')}</span> {previewValue(normalizedLabelsDraft.join(', '))}</div>
                    </div>
                  </div>
                )}
                {notesChanged && (
                  <div className="rounded-md border border-slate-100 bg-slate-50 p-2 text-xs">
                    <div className="mb-1 font-semibold text-slate-700">{t('panel_notes')}</div>
                    <div className="grid gap-1 text-[11px]">
                      <div><span className="font-semibold text-slate-400">{t('edit_before')}</span> {previewValue(session.notes || '')}</div>
                      <div><span className="font-semibold text-blue-500">{t('edit_after')}</span> {previewValue(notesDraft)}</div>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="p-6 border-t border-slate-100 bg-slate-50/30 space-y-2 shrink-0">
        {mode === 'edit' && (
          <div className="grid grid-cols-2 gap-2">
            <button
              type="button"
              onClick={cancelEdit}
              disabled={isEditBusy}
              className="inline-flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white py-2 text-sm font-semibold text-slate-600 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
            >
              <X className="h-4 w-4" />
              {t('btn_cancel')}
            </button>
            <button
              type="button"
              onClick={reviewChanges}
              disabled={!canReview || isEditBusy}
              className="inline-flex items-center justify-center gap-2 rounded-lg bg-blue-600 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <ClipboardCheck className="h-4 w-4" />
              {t('btn_review_changes')}
            </button>
          </div>
        )}
        {mode === 'review' && (
          <div className="grid grid-cols-2 gap-2">
            <button
              type="button"
              onClick={() => setMode('edit')}
              disabled={isEditBusy}
              className="inline-flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white py-2 text-sm font-semibold text-slate-600 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
            >
              <RotateCcw className="h-4 w-4" />
              {t('btn_back_to_edit')}
            </button>
            <button
              type="button"
              onClick={applyChanges}
              disabled={!canReview || isEditBusy}
              className="inline-flex items-center justify-center gap-2 rounded-lg bg-slate-900 py-2 text-sm font-semibold text-white transition-colors hover:bg-black disabled:cursor-not-allowed disabled:opacity-50"
            >
              <ShieldCheck className={cn("h-4 w-4", isSavingEdit && "animate-pulse")} />
              {isSavingEdit ? t('btn_applying_changes') : t('btn_apply_changes')}
            </button>
          </div>
        )}
        <button
          onClick={copyPath}
          disabled={mode !== 'view'}
          className={cn(
            "w-full py-2.5 text-white rounded-lg text-sm font-semibold transition-colors flex items-center justify-center gap-2",
            pathCopyStatus === 'copied' && "bg-emerald-600 hover:bg-emerald-700",
            pathCopyStatus === 'failed' && "bg-red-600 hover:bg-red-700",
            pathCopyStatus === 'idle' && "bg-slate-900 hover:bg-black",
            mode !== 'view' && "cursor-not-allowed opacity-50"
          )}
        >
          <PathCopyIcon className="w-4 h-4" />
          <span aria-live="polite">{copyButtonText}</span>
        </button>
        {!isDeleted ? (
          <button 
            onClick={() => onDelete(session.id)}
            disabled={isBusy || mode !== 'view'}
            className="w-full py-2.5 border border-red-200 text-red-600 bg-white rounded-lg text-sm font-semibold hover:bg-red-50 transition-colors flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
          >
            <Trash2 className="w-4 h-4" />
            {t('btn_delete_archive')}
          </button>
        ) : (
          <button 
            onClick={() => onRestore(session.id)}
            disabled={isBusy || mode !== 'view'}
            className="w-full py-2.5 border border-emerald-200 text-emerald-600 bg-white rounded-lg text-sm font-semibold hover:bg-emerald-50 transition-colors flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
          >
            <Archive className="w-4 h-4" />
            {t('btn_restore')}
          </button>
        )}
      </div>
      {contentEditorOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/40 p-3 backdrop-blur-sm md:p-6"
          onClick={closeContentEditor}
        >
          <div
            className="flex h-[88vh] w-full max-w-6xl flex-col overflow-hidden rounded-xl border border-slate-200 bg-white shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex shrink-0 items-start justify-between gap-4 border-b border-slate-200 bg-slate-50 px-5 py-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Code2 className="h-5 w-5 text-blue-600" />
                  <h3 className="text-lg font-bold text-slate-900">{t('content_editor_title')}</h3>
                  {contentSnapshot && (
                    <span className="rounded border border-slate-200 bg-white px-2 py-0.5 font-mono text-[11px] font-bold uppercase text-slate-500">
                      {contentSnapshot.format}
                    </span>
                  )}
                </div>
                <p className="mt-1 truncate text-xs font-medium text-slate-500" title={session.path}>
                  {session.path}
                </p>
              </div>
              <button
                type="button"
                onClick={closeContentEditor}
                disabled={isSavingContent}
                className="rounded-md p-1.5 text-slate-400 transition-colors hover:bg-slate-200 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                title={t('collab_close')}
                aria-label={t('collab_close')}
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-slate-100 px-5 py-3 text-xs">
              <div
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-semibold",
                  contentValidation.valid
                    ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                    : "border-red-200 bg-red-50 text-red-700"
                )}
              >
                {contentValidation.valid ? <Check className="h-3.5 w-3.5" /> : <AlertCircle className="h-3.5 w-3.5" />}
                {contentValidation.message}
              </div>
              <span className="text-slate-400">
                {t('content_editor_stats', { lines: contentDraftStats.lines, chars: contentDraftStats.chars })}
              </span>
              {contentChanged && (
                <span className="rounded-full bg-blue-50 px-2.5 py-1 font-semibold text-blue-700">
                  {t('content_editor_unsaved')}
                </span>
              )}
            </div>

            {contentError && (
              <div className="mx-5 mt-4 flex shrink-0 items-start gap-2 rounded-lg border border-red-100 bg-red-50 p-3 text-sm text-red-700">
                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{contentError}</span>
              </div>
            )}

            <div className="min-h-0 flex-1 p-5">
              {isLoadingContent ? (
                <div className="flex h-full items-center justify-center rounded-lg border border-dashed border-slate-200 bg-slate-50 text-sm font-medium text-slate-500">
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('content_editor_loading')}
                </div>
              ) : contentEditorMode === 'review' ? (
                <div className="grid h-full min-h-0 gap-4 lg:grid-cols-2">
                  <div className="flex min-h-0 flex-col overflow-hidden rounded-lg border border-slate-200">
                    <div className="border-b border-slate-200 bg-slate-50 px-3 py-2 text-xs font-bold uppercase tracking-wide text-slate-500">
                      {t('content_editor_original')} · {t('content_editor_stats', { lines: contentOriginalStats.lines, chars: contentOriginalStats.chars })}
                    </div>
                    <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words bg-white p-4 font-mono text-xs leading-relaxed text-slate-600">
                      {contentOriginalReview}
                    </pre>
                  </div>
                  <div className="flex min-h-0 flex-col overflow-hidden rounded-lg border border-blue-200">
                    <div className="border-b border-blue-100 bg-blue-50 px-3 py-2 text-xs font-bold uppercase tracking-wide text-blue-700">
                      {t('content_editor_edited')} · {t('content_editor_stats', { lines: contentDraftStats.lines, chars: contentDraftStats.chars })}
                    </div>
                    <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words bg-white p-4 font-mono text-xs leading-relaxed text-slate-800">
                      {contentDraftReview}
                    </pre>
                  </div>
                </div>
              ) : isJsonlRecordEditor ? (
                <div className="grid h-full min-h-0 overflow-hidden rounded-lg border border-slate-200 bg-white lg:grid-cols-[19rem_minmax(0,1fr)]">
                  <div className="flex min-h-0 flex-col border-b border-slate-200 bg-slate-50 lg:border-b-0 lg:border-r">
                    <div className="shrink-0 space-y-2 border-b border-slate-200 px-3 py-2">
                      <div className="flex items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="text-xs font-bold uppercase tracking-wide text-slate-500">
                            {t('content_editor_jsonl_records', { count: jsonlRecords.length })}
                          </div>
                          <div className="text-[11px] font-medium text-slate-400">
                            {t('content_editor_jsonl_record_hint')}
                          </div>
                        </div>
                        <div className="text-xs font-bold uppercase tracking-wide text-slate-500">
                          {t('content_editor_jsonl_search_count', { count: filteredJsonlRecordEntries.length })}
                        </div>
                      </div>
                      <div className="relative">
                        <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-400" />
                        <input
                          value={jsonlSearchQuery}
                          onChange={(event) => setJsonlSearchQuery(event.target.value)}
                          disabled={isSavingContent}
                          className="h-8 w-full rounded-md border border-slate-200 bg-white pl-8 pr-8 text-xs font-medium text-slate-700 outline-none transition-shadow placeholder:text-slate-400 focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-60"
                          placeholder={t('content_editor_jsonl_search_placeholder')}
                        />
                        {jsonlSearchQuery && (
                          <button
                            type="button"
                            onClick={() => setJsonlSearchQuery('')}
                            disabled={isSavingContent}
                            className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                            title={t('content_editor_jsonl_clear_search')}
                            aria-label={t('content_editor_jsonl_clear_search')}
                          >
                            <X className="h-3.5 w-3.5" />
                          </button>
                        )}
                      </div>
                    </div>
                    <div className="min-h-0 flex-1 overflow-auto p-2">
                      {filteredJsonlRecordEntries.length === 0 ? (
                        <div className="flex h-full min-h-40 items-center justify-center rounded-md border border-dashed border-slate-200 bg-white px-4 text-center text-xs font-medium text-slate-400">
                          {t('content_editor_jsonl_no_search_results')}
                        </div>
                      ) : filteredJsonlRecordEntries.map(({ record, index }) => (
                        <div
                          key={`${record.lineNumber}-${index}`}
                          role="button"
                          tabIndex={0}
                          onClick={() => setSelectedJsonlRecordIndex(index)}
                          onKeyDown={(event) => {
                            if (isSavingContent) return;
                            if (event.key === 'Enter' || event.key === ' ') {
                              event.preventDefault();
                              setSelectedJsonlRecordIndex(index);
                            }
                          }}
                          aria-disabled={isSavingContent}
                          className={cn(
                            "mb-1 w-full cursor-pointer rounded-md border px-3 py-2 text-left outline-none transition-colors focus:ring-2 focus:ring-blue-200",
                            isSavingContent && "cursor-not-allowed opacity-60",
                            index === selectedJsonlRecordIndex
                              ? "border-blue-200 bg-blue-50 text-blue-900"
                              : "border-transparent bg-white text-slate-700 hover:border-slate-200 hover:bg-slate-100"
                          )}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <span className="truncate text-xs font-bold">
                              {t('content_editor_jsonl_record', { index: index + 1 })}
                            </span>
                            <div className="flex shrink-0 items-center gap-1.5">
                              <span className="rounded border border-slate-200 bg-white px-1.5 py-0.5 font-mono text-[10px] font-semibold text-slate-400">
                                {t('content_editor_jsonl_line', { line: record.lineNumber })}
                              </span>
                              <button
                                type="button"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  deleteJsonlRecord(index);
                                }}
                                disabled={isSavingContent}
                                className="rounded border border-slate-200 bg-white p-1 text-slate-400 transition-colors hover:border-red-200 hover:bg-red-50 hover:text-red-600 disabled:cursor-not-allowed disabled:opacity-50"
                                title={t('content_editor_jsonl_delete_record')}
                                aria-label={t('content_editor_jsonl_delete_record')}
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </button>
                            </div>
                          </div>
                          <div className="mt-1 truncate text-[11px] font-semibold text-slate-600">
                            {record.title}
                          </div>
                          <div className="mt-0.5 truncate font-mono text-[10px] text-slate-400">
                            {record.subtitle}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                  <div className="flex min-h-0 flex-col">
                    <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-slate-200 bg-white px-4 py-3">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-bold text-slate-900">
                          {selectedJsonlRecord?.title || t('content_editor_jsonl_empty')}
                        </div>
                        {selectedJsonlRecord && (
                          <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] font-semibold text-slate-400">
                            <span>{t('content_editor_jsonl_record', { index: selectedJsonlRecordIndex + 1 })}</span>
                            <span>{t('content_editor_jsonl_line', { line: selectedJsonlRecord.lineNumber })}</span>
                            <span className="rounded border border-slate-200 px-1.5 py-0.5 font-mono uppercase">
                              {selectedJsonlRecord.type}
                            </span>
                          </div>
                        )}
                      </div>
                      <button
                        type="button"
                        onClick={revertJsonlRecordDraft}
                        disabled={!selectedJsonlRecord || selectedJsonlRecord.draft === selectedJsonlRecord.originalDraft || isSavingContent}
                        className="inline-flex items-center justify-center gap-1.5 rounded-md border border-slate-200 bg-white px-3 py-1.5 text-xs font-semibold text-slate-600 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                        {t('content_editor_jsonl_revert_record')}
                      </button>
                    </div>
                    <textarea
                      value={selectedJsonlRecord?.draft || ''}
                      onChange={(event) => updateJsonlRecordDraft(event.target.value)}
                      spellCheck={false}
                      disabled={!selectedJsonlRecord || isSavingContent}
                      className="min-h-0 flex-1 resize-none border-0 bg-slate-950 p-4 font-mono text-xs leading-relaxed text-slate-100 outline-none placeholder:text-slate-500 focus:ring-2 focus:ring-blue-500/30 disabled:cursor-not-allowed disabled:opacity-60"
                      placeholder={t('content_editor_jsonl_empty')}
                    />
                  </div>
                </div>
              ) : (
                <textarea
                  value={contentDraft}
                  onChange={(event) => setContentDraft(event.target.value)}
                  spellCheck={false}
                  disabled={!contentSnapshot || isSavingContent}
                  className="h-full w-full resize-none rounded-lg border border-slate-200 bg-slate-950 p-4 font-mono text-xs leading-relaxed text-slate-100 outline-none transition-shadow placeholder:text-slate-500 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/30 disabled:cursor-not-allowed disabled:opacity-60"
                  placeholder={t('content_editor_placeholder')}
                />
              )}
            </div>

            <div className="flex shrink-0 flex-col gap-3 border-t border-slate-200 bg-slate-50 px-5 py-4 md:flex-row md:items-center md:justify-between">
              <p className="max-w-2xl text-xs leading-relaxed text-slate-500">
                {t('content_editor_footer_hint')}
              </p>
              <div className="flex shrink-0 items-center gap-2">
                {contentEditorMode === 'review' ? (
                  <button
                    type="button"
                    onClick={() => setContentEditorMode('edit')}
                    disabled={isSavingContent}
                    className="inline-flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <RotateCcw className="h-4 w-4" />
                    {t('btn_back_to_edit')}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={closeContentEditor}
                    disabled={isSavingContent}
                    className="inline-flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <X className="h-4 w-4" />
                    {t('btn_cancel')}
                  </button>
                )}
                {contentEditorMode === 'review' ? (
                  <button
                    type="button"
                    onClick={applyContentChanges}
                    disabled={!canReviewContent || isSavingContent}
                    className="inline-flex items-center justify-center gap-2 rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-black disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {isSavingContent ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
                    {isSavingContent ? t('content_editor_saving') : t('content_editor_apply')}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={reviewContentChanges}
                    disabled={!canReviewContent || isSavingContent}
                    className="inline-flex items-center justify-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <ClipboardCheck className="h-4 w-4" />
                    {t('btn_review_changes')}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
