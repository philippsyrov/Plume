#!/bin/bash
# Fixture verifier: empty flags must be filtered out.
set -euo pipefail
grep -q 'flag.length > 0' src/parser.ts
! grep -q 'flag.length >= 0' src/parser.ts
