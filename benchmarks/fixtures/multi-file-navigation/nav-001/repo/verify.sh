#!/bin/bash
# Fixture verifier: the loader must respect MAX_ENTRIES exactly.
set -euo pipefail
grep -q 'raw.slice(0, MAX_ENTRIES)' src/loader.ts
! grep -q 'MAX_ENTRIES + 1' src/loader.ts
