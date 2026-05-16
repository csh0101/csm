import React from 'react';
import { cn } from '../lib/utils';

interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: 'default' | 'active' | 'stale' | 'deleted' | 'outline';
}

export function Badge({ children, variant = 'default', className, ...props }: BadgeProps) {
  const baseClasses = 'inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset';
  
  const variants = {
    default: 'bg-gray-50 text-gray-600 ring-gray-500/10',
    active: 'bg-green-50 text-green-700 ring-green-600/20',
    stale: 'bg-amber-50 text-amber-700 ring-amber-600/20',
    deleted: 'bg-red-50 text-red-700 ring-red-600/10',
    outline: 'text-gray-600 ring-gray-500/20 bg-transparent'
  };

  return (
    <span className={cn(baseClasses, variants[variant], className)} {...props}>
      {children}
    </span>
  );
}
