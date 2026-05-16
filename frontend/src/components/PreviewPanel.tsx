import React, { useEffect, useState } from 'react';
import { Session } from '../types';
import Markdown from 'react-markdown';
import { Archive, Trash2, Check, Copy, FileText, Terminal, X } from 'lucide-react';
import { cn } from '../lib/utils';
import { useI18n } from '../i18n';

interface PreviewPanelProps {
  session: Session | null;
  onClose: () => void;
  onDelete: (id: string) => Promise<void>;
  onRestore: (id: string) => Promise<void>;
  onUpdateLabels: (id: string, labels: string[]) => Promise<void>;
  onUpdateNotes: (id: string, notes: string) => Promise<void>;
  isBusy: boolean;
}

type CopyStatus = 'idle' | 'copied' | 'failed';

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
  isBusy
}: PreviewPanelProps) {
  const { t } = useI18n();
  const [labelInput, setLabelInput] = useState('');
  const [isAddingLabel, setIsAddingLabel] = useState(false);
  const [notesDraft, setNotesDraft] = useState('');
  const [pathCopyStatus, setPathCopyStatus] = useState<CopyStatus>('idle');
  const [resumeCopyStatus, setResumeCopyStatus] = useState<CopyStatus>('idle');

  useEffect(() => {
    setLabelInput('');
    setIsAddingLabel(false);
    setNotesDraft(session?.notes || '');
    setPathCopyStatus('idle');
    setResumeCopyStatus('idle');
  }, [session?.id, session?.notes]);

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
  const addLabel = async () => {
    const label = labelInput.trim();
    if (!label || session.labels.includes(label)) {
      setLabelInput('');
      setIsAddingLabel(false);
      return;
    }

    await onUpdateLabels(session.id, [...session.labels, label]);
    setLabelInput('');
    setIsAddingLabel(false);
  };

  const removeLabel = async (label: string) => {
    await onUpdateLabels(session.id, session.labels.filter((item) => item !== label));
  };

  const saveNotes = async () => {
    await onUpdateNotes(session.id, notesDraft);
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
          <h2 className="font-bold text-slate-900 leading-tight">{t('panel_preview')}</h2>
          <button onClick={onClose} className="text-slate-400 hover:text-red-500 transition-colors">
            <X className="w-5 h-5" />
          </button>
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
          <label className="text-[10px] font-bold text-slate-400 uppercase mb-2 block">{t('panel_content')}</label>
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
          <div className="flex flex-wrap gap-2 mb-2">
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
                <span key={label} className={cn("px-2 py-1 border rounded text-[10px] font-bold flex items-center gap-1 uppercase", colorClass)}>
                  {label}
                  <button
                    disabled={isBusy}
                    onClick={() => removeLabel(label)}
                    className="hover:opacity-70 disabled:cursor-not-allowed"
                  >
                    &times;
                  </button>
                </span>
              );
            })}
            {isAddingLabel ? (
              <input
                autoFocus
                value={labelInput}
                onChange={(event) => setLabelInput(event.target.value)}
                onBlur={addLabel}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') addLabel();
                  if (event.key === 'Escape') setIsAddingLabel(false);
                }}
                className="px-2 py-1 bg-white border border-blue-200 rounded text-[10px] font-medium outline-none w-24"
              />
            ) : (
              <button
                disabled={isBusy}
                onClick={() => setIsAddingLabel(true)}
                className="px-2 py-1 bg-white border border-dashed border-slate-300 text-slate-400 rounded text-[10px] font-medium hover:border-slate-400 transition-colors disabled:cursor-not-allowed"
              >
                {t('btn_add_label')}
              </button>
            )}
          </div>
        </div>

        <div>
          <label className="text-[10px] font-bold text-slate-400 uppercase mb-2 block">{t('panel_notes')}</label>
          <textarea 
            value={notesDraft}
            onChange={(event) => setNotesDraft(event.target.value)}
            className="w-full h-24 p-3 bg-white border border-slate-200 rounded-lg text-xs resize-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 outline-none transition-shadow" 
            placeholder={t('panel_notes_placeholder')}
          ></textarea>
          <button
            disabled={isBusy || notesDraft === (session.notes || '')}
            onClick={saveNotes}
            className="mt-2 px-3 py-1.5 bg-white border border-slate-200 rounded text-xs font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {t('btn_save_notes')}
          </button>
        </div>
      </div>

      <div className="p-6 border-t border-slate-100 bg-slate-50/30 space-y-2 shrink-0">
        <button
          onClick={copyPath}
          className={cn(
            "w-full py-2.5 text-white rounded-lg text-sm font-semibold transition-colors flex items-center justify-center gap-2",
            pathCopyStatus === 'copied' && "bg-emerald-600 hover:bg-emerald-700",
            pathCopyStatus === 'failed' && "bg-red-600 hover:bg-red-700",
            pathCopyStatus === 'idle' && "bg-slate-900 hover:bg-black"
          )}
        >
          <PathCopyIcon className="w-4 h-4" />
          <span aria-live="polite">{copyButtonText}</span>
        </button>
        {!isDeleted ? (
          <button 
            onClick={() => onDelete(session.id)}
            disabled={isBusy}
            className="w-full py-2.5 border border-red-200 text-red-600 bg-white rounded-lg text-sm font-semibold hover:bg-red-50 transition-colors flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
          >
            <Trash2 className="w-4 h-4" />
            {t('btn_delete_archive')}
          </button>
        ) : (
          <button 
            onClick={() => onRestore(session.id)}
            disabled={isBusy}
            className="w-full py-2.5 border border-emerald-200 text-emerald-600 bg-white rounded-lg text-sm font-semibold hover:bg-emerald-50 transition-colors flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
          >
            <Archive className="w-4 h-4" />
            {t('btn_restore')}
          </button>
        )}
      </div>
    </aside>
  );
}
