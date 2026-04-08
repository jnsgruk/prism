mod agent_query;
mod ai_settings;
mod conversations;
mod convert;
mod cost;
mod embeddings;
mod enrichments;
pub mod workspace;

use std::sync::Arc;

use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningService;
use ps_proto::canonical::prism::v1::{
    AskQuestionRequest, AskQuestionResponse, DeleteConversationRequest, DeleteConversationResponse,
    DeleteEnrichmentsByTypeRequest, DeleteEnrichmentsByTypeResponse, FindSimilarRequest,
    FindSimilarResponse, GetAiSettingsRequest, GetAiSettingsResponse, GetConversationRequest,
    GetConversationResponse, GetEmbeddingStatusRequest, GetEmbeddingStatusResponse,
    GetEnrichmentPipelineStatusRequest, GetEnrichmentPipelineStatusResponse,
    GetEnrichmentsByContributionsRequest, GetEnrichmentsByContributionsResponse,
    GetEnrichmentsRequest, GetEnrichmentsResponse, GetUsageSummaryRequest, GetUsageSummaryResponse,
    GetWorkspaceFileRequest, GetWorkspaceFileResponse, ListAiModelsRequest, ListAiModelsResponse,
    ListConversationsRequest, ListConversationsResponse, ListWorkspaceFilesRequest,
    ListWorkspaceFilesResponse, RefreshModelCatalogueRequest, RefreshModelCatalogueResponse,
    RenameConversationRequest, RenameConversationResponse, ResumeStreamRequest,
    ResumeStreamResponse, SaveInsightFromConversationRequest, SaveInsightFromConversationResponse,
    SearchByTextRequest, SearchByTextResponse, SetProviderSecretRequest, SetProviderSecretResponse,
    TestProviderRequest, TestProviderResponse, UpdateAiSettingsRequest, UpdateAiSettingsResponse,
};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use zeroize::Zeroizing;

pub struct ReasoningServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
    workspaces_path: Option<std::path::PathBuf>,
    restate_url: String,
    http_client: reqwest::Client,
}

impl ReasoningServiceImpl {
    pub fn new(
        repos: Repos,
        secret_key: Zeroizing<[u8; 32]>,
        router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
        workspaces_path: Option<std::path::PathBuf>,
        restate_url: String,
    ) -> Self {
        Self {
            repos,
            secret_key,
            router,
            workspaces_path,
            restate_url,
            http_client: reqwest::Client::new(),
        }
    }

    /// Load AI config and provider API keys from the database into the router.
    ///
    /// Called at startup so that provider keys survive server restarts.
    pub async fn load_providers_from_db(&self) {
        ai_settings::load_providers_from_db_impl(self).await;
    }
}

#[tonic::async_trait]
impl ReasoningService for ReasoningServiceImpl {
    type AskQuestionStream =
        tokio_stream::wrappers::ReceiverStream<Result<AskQuestionResponse, Status>>;
    type ResumeStreamStream =
        tokio_stream::wrappers::ReceiverStream<Result<ResumeStreamResponse, Status>>;

    async fn get_ai_settings(
        &self,
        request: Request<GetAiSettingsRequest>,
    ) -> Result<Response<GetAiSettingsResponse>, Status> {
        ai_settings::get_ai_settings(self, request).await
    }

    async fn update_ai_settings(
        &self,
        request: Request<UpdateAiSettingsRequest>,
    ) -> Result<Response<UpdateAiSettingsResponse>, Status> {
        ai_settings::update_ai_settings(self, request).await
    }

    async fn set_provider_secret(
        &self,
        request: Request<SetProviderSecretRequest>,
    ) -> Result<Response<SetProviderSecretResponse>, Status> {
        ai_settings::set_provider_secret(self, request).await
    }

    async fn list_ai_models(
        &self,
        request: Request<ListAiModelsRequest>,
    ) -> Result<Response<ListAiModelsResponse>, Status> {
        ai_settings::list_ai_models(self, request).await
    }

    async fn refresh_model_catalogue(
        &self,
        request: Request<RefreshModelCatalogueRequest>,
    ) -> Result<Response<RefreshModelCatalogueResponse>, Status> {
        ai_settings::refresh_model_catalogue(self, request).await
    }

    async fn test_provider(
        &self,
        request: Request<TestProviderRequest>,
    ) -> Result<Response<TestProviderResponse>, Status> {
        ai_settings::test_provider(self, request).await
    }

    async fn get_usage_summary(
        &self,
        request: Request<GetUsageSummaryRequest>,
    ) -> Result<Response<GetUsageSummaryResponse>, Status> {
        cost::get_usage_summary(self, request).await
    }

    async fn get_enrichments(
        &self,
        request: Request<GetEnrichmentsRequest>,
    ) -> Result<Response<GetEnrichmentsResponse>, Status> {
        enrichments::get_enrichments(self, request).await
    }

    async fn get_enrichments_by_contributions(
        &self,
        request: Request<GetEnrichmentsByContributionsRequest>,
    ) -> Result<Response<GetEnrichmentsByContributionsResponse>, Status> {
        enrichments::get_enrichments_by_contributions(self, request).await
    }

    async fn get_enrichment_pipeline_status(
        &self,
        request: Request<GetEnrichmentPipelineStatusRequest>,
    ) -> Result<Response<GetEnrichmentPipelineStatusResponse>, Status> {
        enrichments::get_enrichment_pipeline_status(self, request).await
    }

    async fn delete_enrichments_by_type(
        &self,
        request: Request<DeleteEnrichmentsByTypeRequest>,
    ) -> Result<Response<DeleteEnrichmentsByTypeResponse>, Status> {
        enrichments::delete_enrichments_by_type(self, request).await
    }

    async fn find_similar(
        &self,
        request: Request<FindSimilarRequest>,
    ) -> Result<Response<FindSimilarResponse>, Status> {
        embeddings::find_similar(self, request).await
    }

    async fn search_by_text(
        &self,
        request: Request<SearchByTextRequest>,
    ) -> Result<Response<SearchByTextResponse>, Status> {
        embeddings::search_by_text(self, request).await
    }

    async fn get_embedding_status(
        &self,
        request: Request<GetEmbeddingStatusRequest>,
    ) -> Result<Response<GetEmbeddingStatusResponse>, Status> {
        embeddings::get_embedding_status(self, request).await
    }

    async fn ask_question(
        &self,
        request: Request<AskQuestionRequest>,
    ) -> Result<Response<Self::AskQuestionStream>, Status> {
        agent_query::ask_question(self, request).await
    }

    async fn resume_stream(
        &self,
        request: Request<ResumeStreamRequest>,
    ) -> Result<Response<Self::ResumeStreamStream>, Status> {
        agent_query::resume_stream(self, request).await
    }

    async fn list_conversations(
        &self,
        request: Request<ListConversationsRequest>,
    ) -> Result<Response<ListConversationsResponse>, Status> {
        conversations::list_conversations(self, request).await
    }

    async fn get_conversation(
        &self,
        request: Request<GetConversationRequest>,
    ) -> Result<Response<GetConversationResponse>, Status> {
        conversations::get_conversation(self, request).await
    }

    async fn delete_conversation(
        &self,
        request: Request<DeleteConversationRequest>,
    ) -> Result<Response<DeleteConversationResponse>, Status> {
        conversations::delete_conversation(self, request).await
    }

    async fn rename_conversation(
        &self,
        request: Request<RenameConversationRequest>,
    ) -> Result<Response<RenameConversationResponse>, Status> {
        conversations::rename_conversation(self, request).await
    }

    async fn save_insight_from_conversation(
        &self,
        request: Request<SaveInsightFromConversationRequest>,
    ) -> Result<Response<SaveInsightFromConversationResponse>, Status> {
        conversations::save_insight_from_conversation(self, request).await
    }

    async fn list_workspace_files(
        &self,
        request: Request<ListWorkspaceFilesRequest>,
    ) -> Result<Response<ListWorkspaceFilesResponse>, Status> {
        workspace::list_workspace_files(self, request).await
    }

    async fn get_workspace_file(
        &self,
        request: Request<GetWorkspaceFileRequest>,
    ) -> Result<Response<GetWorkspaceFileResponse>, Status> {
        workspace::get_workspace_file(self, request).await
    }
}
