use ps_core::models::EnrichmentType;
use ps_reasoning::features::enrichment;
use serde::Serialize;

/// Progress report for the enrichment pipeline (stored as run progress JSON).
#[derive(Serialize)]
pub(super) struct EnrichmentProgress {
    pub(super) phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) review_depth: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sentiment: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) significance: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) topic: Option<TypeProgress>,
    pub(super) status_message: String,
}

#[derive(Serialize, Clone)]
pub(super) struct TypeProgress {
    pub(super) processed: usize,
    pub(super) errors: usize,
}

/// Mutable state accumulated across enrichment batches.
pub(super) struct CycleState {
    pub(super) total_processed: i32,
    pub(super) total_errors: usize,
    pub(super) first_error: Option<String>,
    pub(super) rd_stats: TypeProgress,
    pub(super) se_stats: TypeProgress,
    pub(super) si_stats: TypeProgress,
    pub(super) to_stats: TypeProgress,
    pub(super) progress: EnrichmentProgress,
    pub(super) iteration: u32,
}

impl CycleState {
    pub(super) fn new() -> Self {
        Self {
            total_processed: 0,
            total_errors: 0,
            first_error: None,
            rd_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            se_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            si_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            to_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            progress: EnrichmentProgress {
                phase: "starting".into(),
                review_depth: None,
                sentiment: None,
                significance: None,
                topic: None,
                status_message: "Starting enrichment cycle".into(),
            },
            iteration: 0,
        }
    }

    /// Per-type telemetry (enrichment-row counts). Does NOT update
    /// `total_processed` — that is handled once per iteration by
    /// [`aggregate_iteration`] with distinct contribution counting.
    pub(super) fn aggregate_batch(&mut self, batch: &enrichment::BatchResult) {
        if self.first_error.is_none() {
            self.first_error.clone_from(&batch.first_error);
        }
        self.total_errors += batch.errors;

        let stats = match batch.enrichment_type {
            EnrichmentType::ReviewDepth => &mut self.rd_stats,
            EnrichmentType::Sentiment => &mut self.se_stats,
            EnrichmentType::Significance => &mut self.si_stats,
            EnrichmentType::Topic => &mut self.to_stats,
        };
        stats.processed += batch.processed;
        stats.errors += batch.errors;
    }

    /// Count distinct contributions successfully processed across all types
    /// in an iteration. A `pr_review` can produce 2 enrichment rows and a
    /// large `pull_request` up to 3, but each is one unit of work for the
    /// progress UI.
    pub(super) fn aggregate_iteration(&mut self, batches: &[enrichment::BatchResult]) {
        let mut distinct = std::collections::HashSet::new();
        for b in batches {
            distinct.extend(b.successful_contribution_ids.iter().copied());
        }
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        {
            self.total_processed += distinct.len() as i32;
        }
    }

    pub(super) fn update_progress_after_batch(&mut self, results: &[enrichment::BatchResult]) {
        self.progress.review_depth = Some(self.rd_stats.clone());
        self.progress.sentiment = Some(self.se_stats.clone());
        self.progress.significance = Some(self.si_stats.clone());
        self.progress.topic = Some(self.to_stats.clone());
        self.progress.status_message = format!(
            "Batch complete: {} processed, {} errors ({} total)",
            results.iter().map(|r| r.processed).sum::<usize>(),
            results.iter().map(|r| r.errors).sum::<usize>(),
            self.total_processed,
        );
    }
}
