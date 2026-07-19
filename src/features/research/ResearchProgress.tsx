import { useState } from 'react';
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
  const [detailsOpen, setDetailsOpen] = useState(false);
  const active = ACTIVE_STATUSES.includes(status);
  const currentStep = steps.at(-1);
  const summary = error ?? currentStep?.summary ?? statusCopy(status);
  const logicalTurns = currentStep?.logicalTurns ?? 0;
  const providerCalls = currentStep?.providerCalls ?? 0;

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
      {steps.length > 0 || details.length > 0 ? (
        <div className="plume-research-details">
          <button
            type="button"
            className="plume-research-details-trigger"
            aria-expanded={detailsOpen}
            onClick={() => setDetailsOpen((current) => !current)}
          >
            Details
          </button>
          {detailsOpen ? (
            <div className="plume-research-details-content">
              <p>{logicalTurns} logical {logicalTurns === 1 ? 'turn' : 'turns'} · {providerCalls} model {providerCalls === 1 ? 'call' : 'calls'}</p>
              {steps.length > 0 ? (
                <ol>
                  {steps.map((step) => (
                    <li key={step.phase}>{step.summary} ({step.current}/{step.total})</li>
                  ))}
                </ol>
              ) : null}
              {details.map((detail, index) => <p key={index}>{detail}</p>)}
            </div>
          ) : null}
        </div>
      ) : null}
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
