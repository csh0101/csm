import React from 'react';
import { FilterType } from '../types';
import { useI18n } from '../i18n';
import { 
  LayoutGrid, Clock, AlertCircle, Trash2, Users
} from 'lucide-react';
import { cn } from '../lib/utils';
import { FilterCounts } from '../types';
import appIconUrl from '../../src-tauri/icons/icon.png';

interface SidebarProps {
  activeView: 'sessions' | 'collaboration';
  currentFilter: FilterType;
  onSelectFilter: (filter: FilterType) => void;
  onSelectCollaboration: () => void;
  counts: FilterCounts;
}

interface NavItemProps {
  icon: React.ElementType;
  label: string;
  count?: number;
  isActive?: boolean;
  onClick: () => void;
  isDanger?: boolean;
  isStale?: boolean;
}

function NavItem({ icon: Icon, label, count, isActive, onClick, isDanger, isStale }: NavItemProps) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full flex items-center justify-between py-2 text-sm transition-colors text-left",
        isActive 
          ? "bg-slate-800 text-white border-l-4 border-blue-500 pl-5 pr-6" 
          : "hover:bg-slate-800 pl-6 pr-6",
        isDanger && !isActive && "text-slate-400 hover:text-red-400 hover:bg-slate-800"
      )}
    >
      <div className="flex items-center gap-3">
        <Icon className={cn(
          "w-4 h-4", 
          isDanger ? "text-red-400" : isStale ? "text-amber-500" : (isActive ? "text-white" : "text-slate-400")
        )} />
        <span className={cn(!isActive && !isDanger && !isStale && "text-slate-300")}>{label}</span>
      </div>
      {count !== undefined && (
        <span className={cn(
          "text-[10px] px-1.5 rounded",
          isStale ? "bg-amber-500/20 text-amber-500" : 
          isActive ? "bg-slate-700 text-slate-300" : 
          "bg-slate-700 text-slate-400"
        )}>
          {count}
        </span>
      )}
    </button>
  );
}

export function Sidebar({ activeView, currentFilter, onSelectFilter, onSelectCollaboration, counts }: SidebarProps) {
  const { t, lang, setLang } = useI18n();
  return (
    <aside className="w-full max-h-[42vh] overflow-hidden bg-[#0F172A] text-slate-300 flex flex-col flex-shrink-0 z-10 transition-all font-sans md:h-screen md:max-h-none md:w-64">
      <div className="p-4 border-b border-slate-800 md:p-6">
        <div className="flex items-center gap-3 text-white">
          <img
            src={appIconUrl}
            alt=""
            className="h-8 w-8 rounded-lg object-cover shadow-sm"
            draggable={false}
          />
          <span className="font-bold text-lg tracking-tight">{t('app_title')}</span>
        </div>
      </div>

      <nav className="flex-1 py-2 space-y-1 overflow-y-auto md:py-4">
        <div className="px-6 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('nav_navigation')}</div>
        <NavItem 
          icon={LayoutGrid} 
          label={t('nav_explorer')} 
          count={counts.all}
          isActive={activeView === 'sessions' && currentFilter === 'all'}
          onClick={() => onSelectFilter('all')} 
        />
        <NavItem
          icon={Users}
          label={t('nav_collaboration')}
          isActive={activeView === 'collaboration'}
          onClick={onSelectCollaboration}
        />
        <NavItem 
          icon={Clock} 
          label={t('nav_recent')} 
          count={counts.recent}
          isActive={activeView === 'sessions' && currentFilter === 'recent'}
          onClick={() => onSelectFilter('recent')} 
        />
        
        <div className="px-6 mt-6 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('nav_smart_filters')}</div>
        <NavItem 
          icon={AlertCircle} 
          label={t('nav_stale')} 
          isStale
          count={counts.stale}
          isActive={activeView === 'sessions' && currentFilter === 'stale'}
          onClick={() => onSelectFilter('stale')} 
        />
      </nav>

      <div className="p-3 border-t border-slate-800 md:p-4">
        <button 
          onClick={() => onSelectFilter('deleted')}
          className={cn(
            "w-full flex items-center gap-3 px-4 py-3 rounded-lg transition-all text-left",
            activeView === 'sessions' && currentFilter === 'deleted' ? "bg-red-500/10 text-red-400" : "hover:bg-red-500/10 text-slate-400 hover:text-red-400"
          )}
        >
          <Trash2 className="w-5 h-5" />
          <span className="font-medium">{t('nav_recycle_bin')}</span>
          {counts.deleted > 0 && <span className="ml-auto text-[10px] bg-red-500/20 text-red-400 px-1.5 rounded">{counts.deleted}</span>}
        </button>
        
        <div className="flex items-center justify-between mt-6 px-4">
          <span className="text-xs font-semibold text-slate-500 uppercase tracking-wider">{t('language')}</span>
          <div className="flex gap-3 text-xs">
            <button 
              onClick={() => setLang('en')} 
              className={cn("transition-colors font-medium", lang === 'en' ? 'text-white' : 'text-slate-500 hover:text-slate-300')}
            >
              {t('lang_en')}
            </button>
            <button 
              onClick={() => setLang('zh')} 
              className={cn("transition-colors font-medium", lang === 'zh' ? 'text-white' : 'text-slate-500 hover:text-slate-300')}
            >
              {t('lang_zh')}
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
}
