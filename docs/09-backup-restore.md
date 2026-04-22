# Backup & Restore

## Overview

Prism's backup/restore system enables instance migration and disaster recovery. A backup captures all persistent state — configuration, org structure, activity data, AI enrichments, and optionally workspace files — into a portable `.ps-backup` archive using `pg_dump`. Restoring an archive drops application schemas and runs `pg_restore` to replace the database state.

## Archive Format

`.ps-backup` files are gzipped tar archives with a fixed structure:

1. **`manifest.json`** — format version, export timestamp, app version, `pg_dump` version, backed-up schemas, workspace file/byte counts, encrypted secret key canary
2. **`database.dump`** — `pg_dump` custom-format dump of all application schemas
3. **Workspace files** — raw files under `workspaces/<conversation-uuid>/...` (optional, excluded with `--no-workspaces`)

### Manifest Fields

| Field | Type | Description |
|-------|------|-------------|
| `format_version` | int | Always `2` for `pg_dump`-based archives |
| `schema_version` | int | Database schema version at time of backup |
| `exported_at` | datetime | UTC timestamp of backup creation |
| `app_version` | string | Prism version that created the backup |
| `pg_version` | string | Major version of `pg_dump` used (e.g. `"17"`) |
| `schemas` | string[] | Database schemas included in the dump |
| `exclude_workspaces` | bool | Whether workspace files were excluded |
| `workspace_file_count` | int | Number of workspace files in the archive |
| `workspace_total_bytes` | int | Total size of workspace files |
| `secret_key_canary` | object | Encrypted known plaintext for key validation |
| `table_counts` | object | Empty (retained for forward compatibility) |

## Schemas Included

All application schemas are included in the `pg_dump`:

| Schema | Contents |
|--------|----------|
| config | `source_configs`, `secrets`, `global_settings` |
| org | `people`, `teams`, `platform_identities`, `team_memberships`, `repositories` |
| activity | `contributions`, `ingestion_watermarks`, `pipelines` |
| reasoning | `enrichments`, `embeddings`, `conversations`, `conversation_messages` |
| metrics | `team_snapshots`, `individual_profiles`, `snapshot_sources`, `insight_snapshots`, `insight_snapshot_sources` |
| auth | `users` |

**Excluded tables:** `activity.ingestion_runs` (ephemeral, rebuilt on demand).

**Not in dump** (live in `public` schema or are session-scoped): `sessions`, `etag_cache`, `ai_models`, `enrichment_queue`, `embedding_queue`, `conversation_events`, `conversation_artifacts`, `api_usage`.

## Encryption and `PS_SECRET_KEY`

Source credentials are backed up as AES-256-GCM ciphertext — never decrypted during backup. The `PS_SECRET_KEY` itself is NOT included in the archive.

Restoring to an instance with a different key makes all encrypted credentials unreadable. A **canary** (encrypted known plaintext) in the manifest detects key mismatches before any data is wiped.

**Operator action:** Copy `PS_SECRET_KEY` from the source instance to the target before restoring.

## Architecture

Both backup and restore are offloaded to Kubernetes Jobs running the `ps-backup` container image. This provides complete process isolation — `pg_dump`/`pg_restore` can saturate CPU without affecting ps-server's readiness probe.

```
Client (psctl/UI)
    │
    ▼
ps-server (CreateBackup RPC)
    │ 1. Concurrent backup guard (check active K8s Jobs)
    │ 2. Create K8s Job (ps-backup container, MODE=backup)
    │ 3. Poll Job status until completion
    │ 4. Stream .ps-backup file from backups PVC to client
    │ 5. Delete file from PVC
    │
    ▼
K8s Job → ps-backup (backup mode)
    │ 1. Run pg_dump (custom format, per-schema)
    │ 2. Walk workspace files (if not excluded)
    │ 3. Build gzipped tar archive (manifest + dump + workspaces)
    │ 4. Atomic rename → .ps-backup
```

Restore follows the same Job pattern:

```
Client (psctl/UI)
    │
    ▼
ps-server (RestoreBackup RPC)
    │ 1. Receive archive upload, validate manifest + canary
    │ 2. Stage archive on backups PVC
    │ 3. Create K8s Job (ps-backup container, MODE=restore)
    │ 4. Poll Job status until completion
    │ 5. Find/create admin user, return session token
    │
    ▼
K8s Job → ps-backup (restore mode)
    │ 1. Ensure pgvector extension (psql)
    │ 2. DROP SCHEMA ... CASCADE for all app schemas
    │ 3. pg_restore from database.dump
    │ 4. Wipe existing workspace directories
    │ 5. Extract workspace files from archive
    │ 6. Clean up archive from PVC
```

### `ps-backup` Container

The `ps-backup` binary is a small Rust crate (`crates/ps-backup/`) that runs in two modes controlled by the `MODE` environment variable:

- **`backup`** — runs `pg_dump`, walks workspaces, builds archive
- **`restore`** — drops schemas, runs `pg_restore`, extracts workspaces

The container uses `pgvector/pgvector:pg17` as its base image (same as the database server), guaranteeing that `pg_dump`/`pg_restore` version matches the server exactly. This avoids version mismatch errors that would occur with separately installed PostgreSQL client tools.

### `BackupGenerator` Trait

The `BackupGenerator` trait (`crates/ps-server/src/features/backup/generator.rs`) abstracts Job management:

- **`KubeBackupGenerator`** (production) — creates/polls/cancels K8s Jobs via the kube API
- **`DirectBackupGenerator`** (tests) — runs `pg_dump`/`pg_restore` directly against the test database

This allows integration tests to exercise the full backup/restore flow without a Kubernetes cluster.

### Storage

A dedicated `prism-backups` PVC is mounted on both ps-server and the ps-backup Job:
- ps-backup writes the archive file
- ps-server reads and streams it to the client, then deletes it
- For restore, ps-server stages the uploaded archive on the PVC for the Job to consume

### Concurrent Backup Prevention

ps-server checks for active K8s Jobs with label `app=ps-backup` before creating a new one. If an active job exists, the RPC returns `ALREADY_EXISTS`. The `--force` flag deletes existing jobs before creating a new one.

### Cancellation

A running backup can be cancelled via the `CancelBackup` RPC. Cancellation deletes the active K8s Job using a background deletion policy. Since the Job runs `pg_dump` as a child process, pod deletion terminates the dump.

### Restore: Schema Drop Strategy

Before running `pg_restore`, the restore Job drops all application schemas with `CASCADE`. This is necessary because `pg_dump` excludes certain tables (e.g. `activity.ingestion_runs`) whose foreign key constraints would block `pg_restore --clean` from dropping referenced tables. Dropping schemas with CASCADE removes all objects — including FK-dependent excluded tables — cleanly.

`pg_restore` then recreates schemas and all objects from the dump without needing `--clean`.

## Authentication Model

All backup RPCs live on `BackupService` (defined in `backup.proto`). Auth is enforced by the interceptor and handler:

| RPC | Uninitialised (no users) | Live (users exist) |
|-----|--------------------------|---------------------|
| `CreateBackup` | N/A (no admin exists) | Admin auth required |
| `CancelBackup` | N/A (no admin exists) | Admin auth required |
| `PreviewBackup` | Open | Admin auth required |
| `RestoreBackup` | Open | Admin auth required |

The interceptor uses a `CONDITIONALLY_PUBLIC_METHODS` list. For these RPCs, it queries `any_users_exist()` — if false (fresh instance), the request passes without auth. If true (live instance), normal token validation applies. The handler additionally checks `AuthContext.role == Admin` to reject non-admin tokens.

## Safety Features

| Feature | Description |
|---------|-------------|
| `pg_dump` internal checksums | Custom format includes built-in data integrity checks |
| Secret key canary | Encrypted known plaintext in manifest; mismatch detected before wipe |
| Format version check | v1 JSONL archives are rejected with a clear error message |
| Pre-restore extension check | pgvector extension is ensured before `pg_restore` runs |
| Schema drop before restore | Avoids FK constraint conflicts with excluded tables |

## CLI Usage (`psctl`)

```bash
# Create a backup (requires PS_API_TOKEN or --token)
psctl backup --output /path/to/backup.ps-backup

# Create without workspace files (smaller archive)
psctl backup --output /path/to/backup.ps-backup --no-workspaces

# Restore to a fresh instance (no auth needed)
psctl restore /path/to/backup.ps-backup

# Restore to a live instance (requires PS_API_TOKEN or --token with admin role)
PS_API_TOKEN=<token> psctl restore /path/to/backup.ps-backup
```

The restore command:
1. Uploads the archive and displays a preview (format version, workspace stats, key validation)
2. Validates secret key canary
3. Prompts for confirmation
4. Drops schemas, restores database, extracts workspace files
5. Returns a new session token for the restored admin user

**Security:** Backup archives contain password hashes and encrypted credentials. Store them with appropriate access controls.

## Instance Migration

1. Stop ingestion on the source instance
2. Note the `PS_SECRET_KEY` value from the source environment
3. Create a backup: `psctl backup --output migration.ps-backup`
4. Deploy the target instance with the same `PS_SECRET_KEY`
5. Restore: `psctl restore migration.ps-backup`
6. Re-register Restate workers: `curl -X POST http://localhost:9070/deployments -H 'content-type: application/json' -d '{"uri":"http://ps-workers:9081/","force":true}'`
7. Verify data in the UI, then resume ingestion

## Testing

The `backup_and_restore_roundtrip` integration test in `tests/integration/src/api/backup.rs` exercises the full lifecycle: seeds data across 10+ tables, creates a backup via the `CreateBackup` RPC, previews it (verifying canary validity), restores via the `RestoreBackup` RPC, then queries every seeded table to verify row counts and specific field values match.

The `DirectBackupGenerator` in `tests/integration/src/common/server.rs` runs `pg_dump` and `pg_restore` directly against the test database, avoiding the need for a Kubernetes cluster. It follows the same schema-drop-then-restore pattern as the production ps-backup binary.

Conditional auth tests in the same file verify:
- Preview/restore succeed without auth on a fresh (uninitialised) instance
- Preview/restore return `UNAUTHENTICATED` on a live instance without auth
- Cancellation when no backup is active returns `cancelled=false`

## Known Limitations

- **Non-transactional restore** — failure mid-`pg_restore` leaves the database in an inconsistent state (schemas dropped but not fully restored). Take a manual PostgreSQL backup before restoring to an instance with existing data.
- **Restate state is not migrated** — the target gets a fresh Restate; workers must be re-registered and any in-flight invocations are lost.
- **Ingestion history not preserved** — `activity.ingestion_runs` is excluded (ephemeral operational data).
- **v1 archives not supported** — old JSONL-format `.ps-backup` files are rejected. Use the Prism version that created them to restore, then re-backup with the current version.
