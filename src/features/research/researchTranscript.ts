import type { SessionIdentity } from '../../lib/api/sessions';

export type ResearchArtifactRef = {
  owner: SessionIdentity;
  artifactId: string;
  version: number;
};

export type ResearchTranscriptEntry =
  | ({ kind: 'researchArtifact' } & ResearchArtifactRef)
  | ({ kind: 'researchExport'; fileName: string } & ResearchArtifactRef);
