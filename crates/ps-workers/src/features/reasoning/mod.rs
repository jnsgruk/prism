pub mod agent_reaper;
pub mod agentic_query;
pub mod embedding;
pub mod enrichment;
pub mod insights;
pub mod model_catalogue;
pub mod query_watchdog;

use std::sync::Arc;

use restate_sdk::endpoint::Builder;
use tokio::sync::RwLock;

use crate::infra::SharedState;

/// Bind all reasoning/AI pipeline handlers to the Restate endpoint.
pub fn bind(
    endpoint: Builder,
    state: &SharedState,
    router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
) -> Builder {
    use agent_reaper::{AgentPodReaperHandler, AgentPodReaperHandlerImpl};
    use agentic_query::{AgenticQueryHandler, AgenticQueryHandlerImpl};
    use embedding::{EmbeddingHandler, EmbeddingHandlerImpl};
    use enrichment::{EnrichmentHandler, EnrichmentHandlerImpl};
    use insights::{InsightsHandler, InsightsHandlerImpl};
    use model_catalogue::{ModelCatalogueHandler, ModelCatalogueHandlerImpl};
    use query_watchdog::{QueryWatchdogHandler, QueryWatchdogHandlerImpl};

    let enrichment = EnrichmentHandlerImpl {
        state: state.clone(),
        router: router.clone(),
    };
    let embedding = EmbeddingHandlerImpl {
        state: state.clone(),
        router,
    };
    let insights = InsightsHandlerImpl {
        state: state.clone(),
    };
    let model_catalogue = ModelCatalogueHandlerImpl {
        state: state.clone(),
    };
    let agent_reaper = AgentPodReaperHandlerImpl {
        state: state.clone(),
    };
    let agentic_query = AgenticQueryHandlerImpl {
        state: state.clone(),
    };
    let query_watchdog = QueryWatchdogHandlerImpl {
        state: state.clone(),
    };

    endpoint
        .bind(enrichment.serve())
        .bind(embedding.serve())
        .bind(insights.serve())
        .bind(model_catalogue.serve())
        .bind(agent_reaper.serve())
        .bind(agentic_query.serve())
        .bind(query_watchdog.serve())
}
