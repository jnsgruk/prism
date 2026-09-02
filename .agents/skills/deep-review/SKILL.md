---
name: deep-review
description: Perform a read-only, multi-angle review of a requested directory using parallel sub-agents, then write a consolidated report. Use only when the user explicitly requests a deep or parallel review.
---

# Deep Code Review

Perform a thorough, multi-angle code review of the directory supplied by the user.

## Phase 1: Discover subdirectories

List the immediate subdirectories of `$ARGUMENTS` (1 level deep only). Exclude hidden directories and non-code directories (e.g. `target/`, `node_modules/`, `dist/`, `build/`). If a subdirectory contains no source files, skip it.

If `$ARGUMENTS` itself contains source files but no subdirectories, treat it as a single unit and skip the per-subdirectory fan-out — launch the four review agents directly on `$ARGUMENTS`.

## Phase 2: Fan out per subdirectory

For **each** subdirectory found in Phase 1, spawn one sub-agent with the collaboration tools. Start independent reviews in parallel when concurrency is available.

Each subdirectory agent receives the prompt below (with `{{DIR}}` replaced by the subdirectory path). The subdirectory agent must then launch **four** review agents **in parallel** — one per review category.

---

### Prompt for each subdirectory agent

> You are reviewing the code in `{{DIR}}`. Spawn four agents in parallel — one for each review category below. Each agent should search, read, and analyze the code in `{{DIR}}` recursively. Do not modify any files.
>
> When all four agents complete, combine their findings into a single structured response using the format in the "Report format" section at the end.
>
> #### Agent 1: Language Idioms
>
> Review for idiomatic use of the language(s) in `{{DIR}}`:
>
> - **Type system**: Are types used precisely? Look for overly broad types (`any`, `String` where an enum fits, `HashMap` where a struct fits), missing newtypes, and stringly-typed interfaces.
> - **Pattern matching & control flow**: Prefer `match`/`if let`/`let else` over chains of `if/else`. Flag `unwrap()`/`expect()` outside tests.
> - **Iterators & functional patterns**: Flag manual index loops where `.iter()/.map()/.filter()/.collect()` would be clearer. Look for unnecessary `.clone()` calls.
> - **Macros & derives**: Are standard derives missing? Could a macro reduce boilerplate?
> - **Error handling**: Are errors propagated with `?` or swallowed silently? Are custom error types used appropriately?
> - **Idiomatic API usage**: Are standard library and framework APIs used as intended? Flag reinvented wheels.
>
> #### Agent 2: Structure, Consistency & Readability
>
> Review for code organization:
>
> - **Duplication**: Flag near-duplicate functions, copy-pasted blocks with minor variations, repeated patterns that belong in a shared helper.
> - **Naming**: Are names descriptive and consistent? Flag abbreviations, misleading names, inconsistent naming conventions within the module.
> - **Module organization**: Does the code follow feature-first structure? Are responsibilities clearly separated? Flag god-files (>500 lines) or god-functions (>50 lines).
> - **Dead code**: Unused functions, unreachable branches, commented-out code.
> - **Consistency**: Are similar operations done the same way throughout? Flag style inconsistencies.
> - **Documentation**: Are public APIs documented? Are complex algorithms explained? (Don't flag missing docs on obvious getters/setters.)
>
> #### Agent 3: Security
>
> Review for security concerns:
>
> - **Input validation**: Is user input validated at system boundaries? Look for SQL injection, command injection, path traversal, XSS vectors.
> - **Authentication & authorization**: Are auth checks present where needed? Can they be bypassed?
> - **Secrets handling**: Are secrets logged, exposed in error messages, or stored in plaintext? Are they properly zeroized after use?
> - **Cryptography**: Are crypto primitives used correctly? Flag weak algorithms, hardcoded keys, missing IV/nonce uniqueness.
> - **Dependency risk**: Are there known-vulnerable dependencies? Are permissions overly broad?
> - **Data exposure**: Are sensitive fields excluded from serialization, logging, and API responses?
>
> #### Agent 4: Performance
>
> Review for performance issues:
>
> - **N+1 queries**: Database queries inside loops. Flag any SQL call that executes per-item instead of batched.
> - **Unnecessary allocations**: Repeated `String`/`Vec` allocations in hot paths, `.clone()` where a reference suffices, collecting into a `Vec` only to iterate again.
> - **Missing concurrency**: Independent I/O operations (DB queries, HTTP calls, file reads) run sequentially when they could be concurrent (`join!`, `try_join!`, `Promise.all`).
> - **Unbounded growth**: Collections that grow without limit (missing pagination, no cap on cache size, unbounded channels).
> - **Blocking in async context**: Synchronous I/O or CPU-heavy work on an async runtime without `spawn_blocking`.
> - **Redundant work**: Recomputing values that could be cached, re-reading files, duplicate network calls.
>
> ---
>
> ### Report format
>
> Return your combined findings as structured text in this exact format:
>
> ```
> # Review: {{DIR}}
>
> ## Findings
>
> ### Language Idioms
> - [severity: low|medium|high] file:line — description
> ...
>
> ### Structure & Readability
> - [severity: low|medium|high] file:line — description
> ...
>
> ### Security
> - [severity: low|medium|high] file:line — description
> ...
>
> ### Performance
> - [severity: low|medium|high] file:line — description
> ...
>
> ## Summary
> Brief paragraph: overall health, top 3 most important issues to address.
> ```
>
> If a category has no findings, write "No issues found." under that heading.

---

## Phase 3: Coalesce into a single report

Once all subdirectory agents have returned, combine their outputs into a single report file at `reports/deep-review-<directory>.md` (replace `/` with `-` in the directory name). Use `apply_patch` to create the report.

The report structure:

```markdown
# Deep Review: $ARGUMENTS
_Generated: {{current date}}_

## Critical & High Findings

List ALL high-severity findings from every subdirectory, grouped by category. This is the "fix these first" section.

## All Findings by Directory

### {{subdirectory 1 name}}
(paste the full findings from that subdirectory agent)

### {{subdirectory 2 name}}
(paste the full findings from that subdirectory agent)

...

## Action Plan

Based on the findings above, produce a prioritized action plan:

1. **Immediate** (high severity, easy fix) — list specific changes
2. **Short-term** (high severity, requires design) — list with approach
3. **Backlog** (medium/low severity) — group by theme
```

Ensure the `reports/` directory exists before writing (create it if needed).

## Rules

- Do NOT modify any source files. This is a read-only review.
- Do NOT review generated code (files in `gen/`, `target/`, `node_modules/`, `dist/`).
- Do NOT flag issues in test code unless they represent a security risk or a test that silently passes when it shouldn't.
- Keep findings actionable: every finding must say what file, what line, and what to do about it.
- If a subdirectory is very large (>100 files), the review agents should focus on the most important files (public API surface, entry points, handlers) rather than trying to cover everything.
