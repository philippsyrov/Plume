import { spawnSync } from 'node:child_process';

import { checkRoadmapDocs } from './docs/roadmap-docs.ts';

const root = process.cwd();
const result = checkRoadmapDocs({
  root,
  git: (args) => {
    const command = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
    return { ok: command.status === 0, stdout: command.stdout };
  },
});

for (const error of result.errors) console.error(`error: ${error}`);
for (const warning of result.warnings) console.error(`warning: ${warning}`);

if (result.errors.length > 0) process.exitCode = 1;
