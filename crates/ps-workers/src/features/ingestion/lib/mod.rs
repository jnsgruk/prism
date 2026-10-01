pub mod chunk;
pub mod finalise;
mod lifecycle;
mod orchestration;
mod progress;
mod scope;
pub mod scoped_store;

pub use finalise::enqueue_enrichments;
pub use orchestration::{
    build_ingestion_context, execute_ingestion_chunked, execute_scoped_ingestion,
    load_ingestion_source_config,
};
pub use progress::{IngestionSpec, ProgressTracker, SerFetchResult};
