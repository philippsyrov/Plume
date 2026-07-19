import { useEffect, useRef, type KeyboardEvent, type ReactNode } from 'react';

type ModalDialogProps = {
  labelledBy: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
};

const focusableSelector =
  'button:not([disabled]), summary, [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableControls(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(focusableSelector)).filter((control) => {
    if (control.closest('[hidden]')) return false;
    const closedDetails = control.closest('details:not([open])');
    return closedDetails === null || control === closedDetails.querySelector(':scope > summary');
  });
}

export function ModalDialog({
  labelledBy,
  onClose,
  children,
  className = '',
}: ModalDialogProps) {
  const dialogRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const returnFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const firstControl = dialogRef.current ? focusableControls(dialogRef.current)[0] : undefined;
    (firstControl ?? dialogRef.current)?.focus();
    return () => {
      if (returnFocus?.isConnected) returnFocus.focus();
    };
  }, []);

  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={dialogRef}
        className={`plume-project-settings-window${className ? ` ${className}` : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onKeyDown={(event) => handleKeyDown(event, onClose)}
      >
        {children}
      </section>
    </div>
  );
}

function handleKeyDown(event: KeyboardEvent<HTMLElement>, onClose: () => void): void {
  if (event.key === 'Escape') {
    event.preventDefault();
    onClose();
    return;
  }
  if (event.key !== 'Tab') return;

  const controls = focusableControls(event.currentTarget);
  if (controls.length === 0) {
    event.preventDefault();
    event.currentTarget.focus();
    return;
  }
  const first = controls[0];
  const last = controls[controls.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
