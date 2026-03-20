//! `PgVectorIndex` — bridge between Rig's agent system and pgvector storage.
//!
//! Implements Rig's `VectorStoreIndex` trait, allowing W3's agent builder to use
//! `.dynamic_context(n, pgvector_index)` for RAG-augmented queries.
//!
//! This module is a placeholder for W3 integration. The core similarity queries
//! are handled directly by `ReasoningRepo::find_similar()` and the gRPC API.

// TODO: implement VectorStoreIndex for PgVectorIndex when W3 agent work begins
