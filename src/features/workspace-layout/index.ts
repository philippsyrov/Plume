// D30: barrel for the workspace-layout module. Keeps App.tsx's
// import list tidy and gives a single insertion point if a future
// slice (e.g. D32 drag-anywhere panels) replaces the internals
// without changing the public surface.
export { PanelToggle } from './PanelToggle';
export { ResizeHandle } from './ResizeHandle';
export {
  useWorkspaceLayout,
  workspaceGridTemplate,
  type WorkspaceLayout,
} from './useWorkspaceLayout';
