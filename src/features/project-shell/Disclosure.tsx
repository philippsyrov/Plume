import type { ReactNode } from 'react';

type DisclosureProps = {
  summary: ReactNode;
  children: ReactNode;
  className?: string;
};

export function Disclosure({ summary, children, className }: DisclosureProps) {
  return (
    <details className={`plume-disclosure${className ? ` ${className}` : ''}`}>
      <summary className="plume-disclosure-summary" tabIndex={0}>
        {summary}
      </summary>
      <div className="plume-disclosure-content">{children}</div>
    </details>
  );
}
