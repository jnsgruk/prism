# Authentication & Authorisation

## Scope

Lightweight auth for day one: a single admin user created via a first-run wizard, with session-based access control on all API calls. No multi-user management UI, no MFA, no SSO. The schema and abstractions are designed so these can be added later without reworking the foundation.

## Design Principles

1. **Structure the data properly now.** The `auth` schema supports multiple users, roles, and sessions even though we only create one user initially.
2. **No shortcuts on password storage.** Argon2id from day one — there is no "upgrade later" for password hashes that have already been stored insecurely.
3. **Session tokens, not JWTs.** Opaque tokens stored server-side are simpler, revocable, and don't require signing key management. JWTs are a solution for distributed systems we don't have.
4. **Bearer header, not cookies.** gRPC/Connect clients send `Authorization: Bearer <token>` metadata. This is idiomatic for gRPC, immune to CSRF, and works identically across gRPC-Web, Connect, and any future native gRPC clients.

## Database Schema: `auth`

One of the system's six bounded contexts (see [02-domain-model.md](./02-domain-model.md)).

```sql
CREATE SCHEMA auth;

CREATE TABLE auth.users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,             -- Argon2id PHC string
    role TEXT NOT NULL DEFAULT 'admin',      -- 'admin' for now, future: 'viewer', 'manager', etc.
    is_active BOOLEAN NOT NULL DEFAULT true,
    -- Optional link to the org.people record for this user
    person_id UUID REFERENCES org.people(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE auth.sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,         -- SHA-256 of the session token
    session_type TEXT NOT NULL DEFAULT 'browser', -- 'browser' (login, 7-day expiry) or 'api_token' (no expiry, manually revoked)
    token_name TEXT,                         -- human-readable label, only set for api_token type
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,                  -- NULL for api_tokens (no expiry)
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_agent TEXT,                         -- browser info, for "active sessions" UI later
    ip_address INET
);

CREATE INDEX idx_sessions_user ON auth.sessions(user_id);
CREATE INDEX idx_sessions_expires ON auth.sessions(expires_at);
```

### Why hash the session token?

The client holds the raw token. The database stores `SHA-256(token)`. If the sessions table is ever leaked (SQL injection, backup exposure), the attacker cannot use the hashes to authenticate. This is the same pattern used by GitHub personal access tokens.

### Migration

```
migrations/
├── ...existing...
├── 0007_create_auth_schema.sql     -- schema + tables above
└── ...
```

The `ps-migrate` init container handles this like all other schemas. No seed data — the first-run wizard handles initial user creation.

## Rust Crates

| Crate | Purpose |
|-------|---------|
| `argon2` (RustCrypto) | Argon2id password hashing, pure Rust |
| `password-auth` | Convenience wrapper: `password_auth::generate_hash` / `verify_password` |
| `rand` | Generate 256-bit session tokens (`rand::random::<[u8; 32]>()`) |
| `sha2` | SHA-256 for hashing session tokens before DB storage |
| `hex` or `base64` | Encode tokens for transport |

All of these are already in the RustCrypto ecosystem, no new C dependencies.

## gRPC Auth Flow

### Token Lifecycle

```
1. Client sends Login(username, password)
2. Server verifies password against Argon2id hash
3. Server generates 256-bit random token
4. Server stores SHA-256(token) in auth.sessions with expiry
5. Server returns raw token to client
6. Client stores token in memory (sessionStorage as fallback)
7. Client attaches "Authorization: Bearer <token>" metadata to every RPC
8. On each RPC, interceptor validates token → attaches user context
9. Logout: server deletes session row, client discards token
```

### Interceptor

Use `tonic-middleware` for async session validation (needs DB lookup):

```rust
struct AuthInterceptor {
    db: PgPool,
    /// RPCs that don't require auth
    public_methods: HashSet<&'static str>,
}

#[async_trait]
impl RequestInterceptor for AuthInterceptor {
    async fn intercept(&self, req: Request<Body>) -> Result<Request<Body>, Status> {
        let method = req.uri().path();

        // Allow unauthenticated access to setup and login
        if self.public_methods.contains(method) {
            return Ok(req);
        }

        let token = extract_bearer_token(&req)
            .ok_or_else(|| Status::unauthenticated("missing authorization token"))?;

        let token_hash = sha256_hex(token);

        let session = sqlx::query_as!(
            Session,
            r#"SELECT s.id, s.user_id, s.expires_at, u.role, u.is_active
               FROM auth.sessions s
               JOIN auth.users u ON u.id = s.user_id
               WHERE s.token_hash = $1
                 AND (s.expires_at IS NULL OR s.expires_at > now())
                 AND u.is_active"#,
            token_hash
        )
        .fetch_optional(&self.db)
        .await
        .map_err(|_| Status::internal("auth lookup failed"))?
        .ok_or_else(|| Status::unauthenticated("invalid or expired session"))?;

        // Touch last_active_at (fire-and-forget, don't block the request)
        let db = self.db.clone();
        let session_id = session.id;
        tokio::spawn(async move {
            let _ = sqlx::query!(
                "UPDATE auth.sessions SET last_active_at = now() WHERE id = $1",
                session_id
            )
            .execute(&db)
            .await;
        });

        // Attach user context via request extensions
        // (accessible in service handlers)
        req.extensions_mut().insert(AuthContext {
            user_id: session.user_id,
            role: session.role,
        });

        Ok(req)
    }
}
```

### Public (Unauthenticated) RPCs

Only accessible without a valid session:

| RPC | Purpose |
|-----|---------|
| `AuthService.Login` | Authenticate with username/password, receive session token |
| `AuthService.GetSetupStatus` | Returns whether initial setup is complete (any admin user exists) |
| `AuthService.CompleteSetup` | Create the initial admin user — only callable when no users exist |
| `AuthService.PreviewBackup` | Inspect a backup file's contents before restoring — only callable when no users exist |
| `AuthService.RestoreBackup` | Restore full system state from a backup file — only callable when no users exist |

Everything else requires a valid session.

## Proto Definitions

```protobuf
service AuthService {
  // Check if the system has been set up (any admin exists)
  rpc GetSetupStatus(GetSetupStatusRequest) returns (GetSetupStatusResponse);

  // Create the initial admin user — fails if any user already exists
  rpc CompleteSetup(CompleteSetupRequest) returns (CompleteSetupResponse);

  // Inspect a backup file before restoring — only callable when no users exist
  rpc PreviewBackup(stream PreviewBackupRequest) returns (PreviewBackupResponse);

  // Restore full system state from a backup — only callable when no users exist
  rpc RestoreBackup(stream RestoreBackupRequest) returns (RestoreBackupResponse);

  // Authenticate and receive a session token
  rpc Login(LoginRequest) returns (LoginResponse);

  // Invalidate the current session
  rpc Logout(LogoutRequest) returns (LogoutResponse);

  // Get the currently authenticated user
  rpc GetCurrentUser(GetCurrentUserRequest) returns (GetCurrentUserResponse);
}

message GetSetupStatusRequest {}
message GetSetupStatusResponse {
  bool setup_complete = 1;
}

message CompleteSetupRequest {
  string username = 1;
  string display_name = 2;
  string password = 3;
}
message CompleteSetupResponse {
  string session_token = 1;  // Log them in immediately after setup
}

message LoginRequest {
  string username = 1;
  string password = 2;
}
message LoginResponse {
  string session_token = 1;
  google.protobuf.Timestamp expires_at = 2;
}

message LogoutRequest {}
message LogoutResponse {}

message GetCurrentUserRequest {}
message GetCurrentUserResponse {
  string user_id = 1;
  string username = 2;
  string display_name = 3;
  string role = 4;
}

// --- Backup/Restore (setup-time only) ---

message PreviewBackupRequest {
  bytes chunk = 1;  // streamed file upload
}
message PreviewBackupResponse {
  int32 schema_version = 1;              // migration number the backup was taken from
  google.protobuf.Timestamp exported_at = 2;
  map<string, int32> table_counts = 3;   // e.g. {"contributions": 3241, "people": 127, ...}
  repeated string source_names = 4;      // configured data sources in the backup
  map<string, string> watermarks = 5;    // source_name → watermark_value (latest cursor per source)
}

message RestoreBackupRequest {
  bytes chunk = 1;  // streamed file upload
}
message RestoreBackupResponse {
  string session_token = 1;              // log the user in as the restored admin
  google.protobuf.Timestamp expires_at = 2;
  map<string, int32> tables_restored = 3; // summary of what was restored
}
```

## First-Run Wizard (Frontend)

### Detection

On app load, the frontend calls `GetSetupStatus`. If `setup_complete = false`, redirect to `/setup`.

```typescript
// lib/auth.ts — called from root layout
export async function checkSetupStatus(): Promise<boolean> {
  const res = await authClient.getSetupStatus({});
  return res.setupComplete;
}
```

### `/setup` Page

The setup page presents two paths: create a fresh instance or restore from a previous backup.

```
┌─────────────────────────────────────┐
│  Welcome to Prism      │
│                                     │
│  [Create admin account]             │
│  [Restore from backup]              │
│                                     │
└─────────────────────────────────────┘
```

**Path 1: Create admin account** — the default path for a brand new instance:

1. **Username** — pre-filled with `admin`, editable
2. **Display name** — e.g. "Jon Seymour"
3. **Password** + **Confirm password** — client-side validation (minimum 12 characters)
4. Submit calls `CompleteSetup`
5. On success, store the returned session token and redirect to the dashboard

**Path 2: Restore from backup** — for bootstrapping from a previous instance's state:

1. **File upload** — select a `.ps-backup` file (produced by the admin UI's "Download Backup" action)
2. File is streamed to `AuthService.PreviewBackup` — the UI shows a summary: schema version, export date, row counts per table, configured sources, watermark positions
3. User reviews the summary and clicks **Restore**
4. File is streamed (again) to `AuthService.RestoreBackup`
5. Server runs migrations, then restores all data (config, org, activity, metrics, auth users). Encrypted secrets are restored as-is (assumes same `PS_SECRET_KEY`).
6. On success, the response includes a session token for the restored admin user — the UI stores it and redirects to the dashboard
7. The instance is now fully operational with all previous data, watermarks, and configuration. Ingestion resumes from the last watermark on its next scheduled run.

The setup page is only accessible when no admin exists. If someone navigates to `/setup` after setup is complete, redirect to `/login`.

### `/login` Page

Simple username + password form. Calls `Login`, stores the token, redirects to dashboard.

### Token Storage (Frontend)

Store the session token in a module-scoped variable (in-memory). Fall back to `sessionStorage` so it survives page refreshes within a tab but is cleared when the tab closes.

```typescript
// lib/session.ts
let token: string | null = null;

export function setToken(t: string) {
  token = t;
  sessionStorage.setItem('ps_session', t);
}

export function getToken(): string | null {
  if (!token) {
    token = sessionStorage.getItem('ps_session');
  }
  return token;
}

export function clearToken() {
  token = null;
  sessionStorage.removeItem('ps_session');
}
```

The Connect transport interceptor attaches the token to every RPC:

```typescript
// lib/api.ts
import { createConnectTransport } from "@connectrpc/connect-web";
import { getToken } from "./session";

export const transport = createConnectTransport({
  baseUrl: "/api",
  interceptors: [
    (next) => async (req) => {
      const token = getToken();
      if (token) {
        req.header.set("Authorization", `Bearer ${token}`);
      }
      return next(req);
    },
  ],
});
```

### Handling 401s

If any RPC returns `UNAUTHENTICATED`, the frontend clears the token and redirects to `/login`. A global error handler in the Connect transport interceptor handles this.

## Session Policy

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Token length | 256 bits (32 bytes) | Sufficient entropy against brute force |
| Token encoding | Base64url | URL-safe, compact |
| Session expiry | 7 days | Single user, low risk, convenience over security |
| Idle timeout | None (initially) | Simplicity; add later if needed |
| Max sessions per user | Unlimited | No reason to limit for single user |
| Session cleanup | Cron job or on login | Delete expired rows periodically |

## Security Considerations

### Password Requirements

Minimum 12 characters. No complexity rules (NIST 800-63B recommends against them). The UI should show a strength indicator but not block on anything other than length.

### Rate Limiting on Login

Not implemented initially (single user, not internet-facing). When needed, add a simple counter in the `auth.users` table:

```sql
ALTER TABLE auth.users ADD COLUMN failed_login_count INTEGER DEFAULT 0;
ALTER TABLE auth.users ADD COLUMN locked_until TIMESTAMPTZ;
```

Lock the account for 1 minute after 5 failures, 5 minutes after 10, etc. Reset on successful login.

### CORS

Configure `tonic-web` with a strict CORS policy. Only allow the frontend origin. Never `Access-Control-Allow-Origin: *` with credentials.

### Audit Trail

For now, session creation/deletion is implicitly logged by the `created_at` column on sessions. A full audit log (`auth.audit_events` table) can be added later when multi-user is needed.

## Backup & Restore

### Motivation

During development, the k8s cluster may be brought up and down frequently, potentially destroying the database. Rather than re-fetching data from external APIs each time (slow, rate-limited, wasteful), the system supports exporting and restoring full state.

### What's included in a backup

| Schema | Tables included | Excluded |
|--------|----------------|----------|
| `config` | `source_configs`, `secrets` (encrypted bytes as-is), `global_settings` | — |
| `org` | `people`, `platform_identities`, `teams`, `team_memberships`, `repositories` | `repo_scans`, `repo_scan_results` (Phase 3+) |
| `activity` | `contributions`, `ingestion_watermarks`, `etag_cache` | `ingestion_runs` (history, not needed for resumption) |
| `metrics` | `team_snapshots`, `individual_profiles`, `snapshot_sources` | — |
| `auth` | `users` (with password hashes) | `sessions` (ephemeral) |

Restate's internal RocksDB state is **not** included — it rediscovers work from the PostgreSQL watermarks on the next scheduled run.

### Assumption: same `PS_SECRET_KEY`

Encrypted secrets in `config.secrets` are exported as raw encrypted bytes. The target instance must use the same `PS_SECRET_KEY` environment variable. This is acceptable for dev workflows where the key is a static value checked into `k8s/secrets.yaml`.

### Backup format

A single `.ps-backup` file containing:
- **Manifest** (JSON): schema version (migration number), export timestamp, table row counts, `psctl` / app version
- **Table data** (JSONL per table): one JSON object per row, serialized through the application's serde types for version awareness

Packaged as a gzipped tar archive. The manifest is always the first entry so it can be read without extracting the entire file.

### Export flow (authenticated — admin UI)

The admin settings page includes a **"Download Backup"** button. Clicking it calls `AdminService.CreateBackup`, which streams the backup file to the browser as a download. See [proto definitions](#proto-definitions) for the `AdminService` definition.

```protobuf
// In admin.proto (authenticated)
service AdminService {
  // Stream a full state backup to the client
  rpc CreateBackup(CreateBackupRequest) returns (stream CreateBackupResponse);

  // Generate a long-lived API token for CLI/automation access
  rpc CreateApiToken(CreateApiTokenRequest) returns (CreateApiTokenResponse);

  // List active API tokens (metadata only, never the token value)
  rpc ListApiTokens(ListApiTokensRequest) returns (ListApiTokensResponse);

  // Revoke an API token
  rpc RevokeApiToken(RevokeApiTokenRequest) returns (RevokeApiTokenResponse);
}

message CreateBackupRequest {}
message CreateBackupResponse {
  bytes chunk = 1;  // streamed file download
}

message CreateApiTokenRequest {
  string name = 1;  // human-readable label, e.g. "psctl-dev", "ci-pipeline"
}
message CreateApiTokenResponse {
  string token_id = 1;
  string token = 2;     // raw token — shown ONCE, never retrievable again
  string name = 3;
}

message ListApiTokensRequest {}
message ListApiTokensResponse {
  repeated ApiTokenInfo tokens = 1;
}
message ApiTokenInfo {
  string token_id = 1;
  string name = 2;
  google.protobuf.Timestamp created_at = 3;
  google.protobuf.Timestamp last_used_at = 4;  // null if never used
}

message RevokeApiTokenRequest {
  string token_id = 1;
}
message RevokeApiTokenResponse {}
```

### Restore flow (unauthenticated — setup wizard)

See the [first-run wizard](#setup-page) section above. `PreviewBackup` and `RestoreBackup` are public RPCs on `AuthService`, gated by the same "no users exist" check as `CompleteSetup`.

The restore process:
1. Runs pending migrations (ensures schema is at least as new as the backup)
2. Validates manifest schema version against current migration version
3. Upserts all rows using the application's existing upsert logic (idempotent, safe to re-run)
4. Creates a session for the restored admin user and returns the token

## API Tokens (for `psctl` and automation)

Long-lived API tokens provide non-interactive authentication for the `psctl` CLI tool and potential CI integrations.

### How they work

API tokens reuse the existing session infrastructure — they're stored in `auth.sessions` with a `session_type` discriminator column and no expiry:

```sql
ALTER TABLE auth.sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'browser';
-- 'browser' = normal login session (7-day expiry)
-- 'api_token' = long-lived CLI/automation token (no expiry, manually revoked)

ALTER TABLE auth.sessions ADD COLUMN token_name TEXT;
-- Human-readable label, only set for api_token type
```

This avoids a separate table — API tokens use the same `token_hash` lookup, the same `AuthContext`, and the same Bearer auth mechanism as browser sessions. The auth interceptor doesn't need to change.

### Lifecycle

1. Admin opens the **System** tab in the admin UI
2. Clicks "Create API Token", provides a name (e.g. "psctl-dev")
3. Server generates a 256-bit random token, stores `SHA-256(token)` in `auth.sessions` with `session_type = 'api_token'` and `expires_at = NULL`
4. The raw token is returned **once** — the UI shows it in a copy-able field with a warning that it won't be shown again
5. The admin exports it: `export PS_API_TOKEN=ps_...`
6. `psctl` reads `PS_API_TOKEN` and sends it as `Authorization: Bearer <token>` on every RPC

### Token format

API tokens are prefixed with `ps_` followed by base64url-encoded random bytes, making them easy to identify in logs and credential scanners:

```
ps_dGhpcyBpcyBhIHRlc3QgdG9rZW4gZm9yIGRldg...
```

### Revocation

The admin UI lists active API tokens (name, created_at, last_used_at — never the token value). Tokens can be revoked individually, which deletes the session row.

### Security notes

- API tokens have the same permissions as the admin user who created them (single-user system, so this is effectively full access)
- `last_used_at` is updated on each use (same fire-and-forget pattern as browser sessions)
- Tokens have no expiry by default — revocation is the only way to invalidate them
- When multi-user is added later, tokens should be scoped to the creating user's permissions

## What This Enables Later

The schema and abstractions are designed to support these without reworking the foundation:

| Future Feature | What's Already in Place |
|----------------|------------------------|
| **Multiple users** | `auth.users` table, roles column, session-per-user |
| **Role-based access** | `role` field on users, `AuthContext.role` in interceptor — add policy checks per RPC |
| **OIDC/SSO** | Add `auth_provider` + `external_id` columns to `auth.users`; swap password verification for token validation in login flow. The `openidconnect` crate handles the protocol. |
| **MFA/Passkeys** | Add `auth.mfa_credentials` table; insert a challenge step between password verification and session creation |
| **Scoped API tokens** | API tokens already exist; add a `permissions` JSONB column for fine-grained access control |
| **Per-person visibility** | `AuthContext` already carries `user_id`; link to `org.people` via `person_id` on `auth.users`; add row-level filtering in queries |
| **Active sessions UI** | `user_agent` and `ip_address` already captured on sessions; API tokens listed separately with name and last_used_at |

## Implementation Sequencing

This slots into **Phase 1, Workstream 2** (backend scaffolding) since auth is infrastructure that everything else depends on:

1. Add `0007_create_auth_schema.sql` migration
2. Add `ps-core/src/auth/` module: password hashing, token generation, session management
3. Add `AuthService` to proto definitions and implement the RPCs
4. Add the auth interceptor to the tonic server
5. Frontend: setup page, login page, token storage, transport interceptor, 401 handling
6. Wire the first-run check into the root layout

The setup wizard naturally becomes the first thing you see when you deploy a fresh instance.
