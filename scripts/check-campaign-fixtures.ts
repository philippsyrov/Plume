import { checkCampaignFixtures } from './docs/campaign-fixtures.ts';

const result = checkCampaignFixtures({ root: process.cwd() });

for (const error of result.errors) console.error(`error: ${error}`);
for (const warning of result.warnings) console.error(`warning: ${warning}`);

if (result.errors.length > 0) process.exitCode = 1;
