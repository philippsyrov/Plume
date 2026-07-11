#!/bin/bash
# Fixture verifier: passes only when the off-by-one loop bound is
# fixed. Pure grep — no network, no installs, no repo access.
set -euo pipefail
grep -q 'i < items.length' src/counter.ts
! grep -q 'i <= items.length' src/counter.ts
