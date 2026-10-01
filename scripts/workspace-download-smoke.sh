#!/usr/bin/env bash
# Isolated Ask download acceptance: PostgreSQL, temporary workspaces and Connect.
# No production conversations, agent pods, PVC files or journals are modified.
set -euo pipefail

prism_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$prism_root"

# Ensure nextest owns an isolated test database even if the caller normally uses
# Tilt's DATABASE_URL. The setup script provisions its own template database.
unset DATABASE_URL PS_TEST_TEMPLATE
mise exec -- cargo build --bin setup-test-db
mise exec -- cargo nextest run -p ps-server -p ps-integration -E \
  'test(workspace) | test(answer_files)'

cd -- "$prism_root/frontend"
mise exec -- bun run test --run \
  views/ask \
  lib/hooks/use-conversations.test.ts \
  views/login
