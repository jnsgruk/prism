use ps_proto::canonical::prism::v1::{
    Enrichment as ProtoEnrichment, SimilarItem as ProtoSimilarItem,
};

use super::super::common::{
    contribution_state_to_proto, contribution_type_to_proto, enrichment_type_to_proto,
    platform_to_proto, to_timestamp,
};

pub fn similar_to_proto(s: ps_core::repo::reasoning::SimilarContribution) -> ProtoSimilarItem {
    let (platform, platform_instance) = platform_to_proto(&s.platform);
    ProtoSimilarItem {
        contribution_id: s.contribution_id.to_string(),
        title: s.title.unwrap_or_default(),
        platform,
        contribution_type: contribution_type_to_proto(&s.contribution_type),
        state: contribution_state_to_proto(s.state.as_deref().unwrap_or("")),
        platform_instance,
        author_name: s.author_name.unwrap_or_default(),
        external_url: s.external_url.unwrap_or_default(),
        distance: s.distance,
        created_at: Some(to_timestamp(s.created_at)),
    }
}

pub fn enrichment_to_proto(e: ps_core::repo::reasoning::EnrichmentRecord) -> ProtoEnrichment {
    ProtoEnrichment {
        id: e.id.to_string(),
        contribution_id: e.contribution_id.to_string(),
        enrichment_type: enrichment_type_to_proto(e.enrichment_type),
        value_json: e.value.to_string(),
        model_name: e.model_name,
        confidence: e.confidence,
        input_hash: e.input_hash,
        input_preview: e.input_preview,
        created_at: Some(to_timestamp(e.created_at)),
    }
}
