import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServerStatus } from '../providers/useMlxServers';

export function ChatModelSelector({
  selected,
  mlxStatus,
  onClear,
  onStop,
}: {
  selected: SelectedModel | null;
  mlxStatus: MlxServerStatus | null;
  onClear: () => void;
  onStop: (() => void) | undefined;
}) {
  if (selected === null) {
    return (
      <div className="plume-chat-model-selector" aria-label="Current model">
        <span className="plume-chat-model-empty">No model selected</span>
      </div>
    );
  }

  const running = mlxStatus?.kind === 'running' ? mlxStatus.handle : null;
  const isBusy = mlxStatus?.kind === 'starting' || mlxStatus?.kind === 'stopping';

  return (
    <div className="plume-chat-model-selector" aria-label="Current model">
      <span className="plume-chat-model-label">Model</span>
      <span className="plume-chat-model-provider">{selected.providerDisplayName}</span>
      <span className="plume-chat-model-name" title={selected.modelId}>{selected.modelId}</span>
      {running ? (
        <span className="ink-badge plume-chat-model-port" title={`mlx-lm bound to 127.0.0.1:${running.port} (pid ${running.pid})`}>
          port {running.port}
        </span>
      ) : null}
      {isBusy ? (
        <span className="plume-chat-model-status" role="status">
          {mlxStatus.kind === 'starting' ? 'starting…' : 'stopping…'}
        </span>
      ) : null}
      {running && onStop ? (
        <button type="button" className="ink-button plume-chat-model-stop" onClick={onStop}>Stop</button>
      ) : null}
      <button
        type="button"
        className="ink-button plume-chat-model-clear"
        onClick={onClear}
        aria-label={`Clear selected model ${selected.providerDisplayName} ${selected.modelId}`}
      >
        Change
      </button>
    </div>
  );
}
