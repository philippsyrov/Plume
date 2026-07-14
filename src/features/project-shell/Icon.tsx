import type { ReactNode, SVGProps } from 'react';

export const ICON_NAMES = [
  'chat',
  'search',
  'library',
  'project',
  'files',
  'settings',
  'help',
  'sidebar-collapse',
  'sidebar-expand',
  'more',
  'plus',
  'close',
  'browser',
  'knowledge',
  'benchmarks',
  'terminal',
  'chevron-down',
] as const;

export type IconName = (typeof ICON_NAMES)[number];

type IconProps = Omit<SVGProps<SVGSVGElement>, 'children' | 'name'> & {
  name: IconName;
  size?: number;
};

export function Icon({
  name,
  size = 16,
  'aria-label': accessibleLabel,
  ...props
}: IconProps) {
  return (
    <svg
      {...props}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      role={accessibleLabel ? 'img' : undefined}
      aria-label={accessibleLabel}
      aria-hidden={accessibleLabel ? undefined : true}
    >
      {iconBody(name)}
    </svg>
  );
}

function iconBody(name: IconName): ReactNode {
  switch (name) {
    case 'chat':
      return <path d="M5 5.5h14v10H9l-4 3v-13Z" />;
    case 'search':
      return <><circle cx="10.5" cy="10.5" r="5.5" /><path d="m15 15 4 4" /></>;
    case 'library':
      return <><path d="M5 4v16M10 4v16M15 5l4-1 2 15-4 1-2-15Z" /></>;
    case 'project':
      return <><path d="M3.5 6.5h6l2 2h9v10h-17v-12Z" /><path d="M3.5 9h17" /></>;
    case 'files':
      return <><path d="M7 3.5h7l4 4v13H7v-17Z" /><path d="M14 3.5v4h4" /></>;
    case 'settings':
      return <><path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" /></>;
    case 'help':
      return <><circle cx="12" cy="12" r="9" /><path d="M9.8 9a2.4 2.4 0 1 1 3.2 2.3c-.7.3-1 .8-1 1.7M12 17h.01" /></>;
    case 'sidebar-collapse':
      return <><rect x="3.5" y="4" width="17" height="16" rx="2" /><path d="M9 4v16m7-11-3 3 3 3" /></>;
    case 'sidebar-expand':
      return <><rect x="3.5" y="4" width="17" height="16" rx="2" /><path d="M9 4v16m4-11 3 3-3 3" /></>;
    case 'more':
      return <><circle cx="5" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="19" cy="12" r="1" fill="currentColor" stroke="none" /></>;
    case 'plus':
      return <path d="M12 5v14M5 12h14" />;
    case 'close':
      return <path d="m6 6 12 12M18 6 6 18" />;
    case 'browser':
      return <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="M3.5 8.5h17M7 6.5h.01M10 6.5h.01" /></>;
    case 'knowledge':
      return <><path d="M4 5.5c3.5-1 6-.2 8 1.5v12c-2-1.7-4.5-2.5-8-1.5v-12ZM20 5.5c-3.5-1-6-.2-8 1.5v12c2-1.7 4.5-2.5 8-1.5v-12Z" /></>;
    case 'benchmarks':
      return <><path d="M5 19V9M12 19V5M19 19v-7" /><path d="M3 19h18" /></>;
    case 'terminal':
      return <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="m7 9 3 3-3 3m6 0h4" /></>;
    case 'chevron-down':
      return <path d="m6 9 6 6 6-6" />;
  }
}
