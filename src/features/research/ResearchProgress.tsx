import type { ResearchRunStatus, ResearchStep } from './useResearchRun';

type ResearchProgressProps = {
  status: ResearchRunStatus;
  steps: ResearchStep[];
  details: string[];
  error: string | null;
  onStop: () => void;
};

const ACTIVE_STATUSES: ResearchRunStatus[] = ['starting', 'running', 'stopping'];

export function ResearchProgress({ status, steps, details, error, onStop }: ResearchProgressProps) {
  const active = ACTIVE_STATUSES.includes(status);
  const currentStep = steps.at(-1);
  const summary = error ?? (active ? currentStep?.summary : null) ?? statusCopy(status);
  void details;

  return (
    <section className="plume-research-progress" aria-label="Research progress">
      <div className="plume-research-progress-row">
        <div role="status" aria-live="polite">{summary}</div>
        {active ? (
          <button type="button" className="ink-button" onClick={onStop}>
            Stop research
          </button>
        ) : null}
      </div>
    </section>
  );
}

function statusCopy(status: ResearchRunStatus): string {
  switch (status) {
    case 'starting': return 'Preparing research…';
    case 'stopping': return 'Stopping research…';
    case 'stopped': return 'Research stopped.';
    case 'complete': return 'Research note ready.';
    case 'needsReview': return 'Draft ready for citation review.';
    case 'error': return 'Research could not finish.';
    default: return 'Research is ready.';
  }
}
