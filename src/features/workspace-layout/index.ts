// D30: barrel for the workspace-layout module. Keeps App.tsx's
// import list tidy and gives a single insertion point if a future
// slice (drag-anywhere layout tree) replaces the internals without
// changing the public surface. D32 added the inner-panel pieces
// (`useInnerPanels`, `InnerToggleStrip`, `EmptyColumn`).
export { PanelToggle } from './PanelToggle';
export { ResizeHandle } from './ResizeHandle';
export {
  useWorkspaceLayout,
  workspaceGridTemplate,
  type WorkspaceLayout,
} from './useWorkspaceLayout';
export { useInnerPanels, type InnerPanels } from './useInnerPanels';
export { InnerToggleStrip, type InnerToggleItem } from './InnerToggleStrip';
export { EmptyColumn } from './EmptyColumn';
