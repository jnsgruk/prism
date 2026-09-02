# Migration Rules

- Use sequential numbered SQL files in `migrations/`.
- Migrations run through the `ps-migrate` init container; application binaries never run them.
- After changing migrations, run `cargo sqlx prepare --workspace` and commit `.sqlx/` changes separately as `chore: update sqlx query cache`.
- Use type-safe `sqlx::query!`, `sqlx::query_as!`, or `sqlx::query_scalar!` macros in repository code; never add runtime string queries.
