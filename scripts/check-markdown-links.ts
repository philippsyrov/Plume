import { execFileSync } from 'node:child_process';

import { checkMarkdownLinks } from './docs/markdown-links.ts';

const root = process.cwd();
const trackedMarkdown = execFileSync('git', ['ls-files', '*.md'], { encoding: 'utf8' })
  .split('\n')
  .filter((path) => path.length > 0);
const issues = checkMarkdownLinks(root, trackedMarkdown);

for (const issue of issues) console.error(`${issue.source}: ${issue.message}`);

if (issues.length > 0) process.exitCode = 1;
