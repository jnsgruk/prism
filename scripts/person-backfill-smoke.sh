#!/usr/bin/env bash
# Reproducible release acceptance with real PostgreSQL and Restate coordination.
# Each test owns its database, provider fixtures and ephemeral Restate container.
# No Tilt data, configured credentials or cluster journals are modified.
set -euo pipefail

prism_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$prism_root"

# Let nextest provision the isolated PostgreSQL fixture even when the caller has
# a local development DATABASE_URL configured.
unset DATABASE_URL PS_TEST_TEMPLATE
mise exec -- cargo build --bin setup-test-db
mise exec -- cargo nextest run -p ps-integration --test-threads 1 -E \
  'test(org_manual) | test(jira_lookup) | test(person_backfill_release) | test(pipeline_preflight) | test(scoped_storage) | test(scoped_enrichment_history) | test(github_person) | test(jira_person) | test(discourse_person) | test(ongoing_) | test(metrics::historical) | test(scoped_release) | test(scoped_chunks) | test(scoped_coordinator) | test(github_review_recovery)'
cd -- "$prism_root/frontend"
mise exec -- bun run test --run \
  views/admin/components/person-dialogs.test.tsx \
  views/admin/components/person-backfill-dialog.test.tsx
