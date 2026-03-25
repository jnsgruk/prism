use ps_proto::canonical::prism::v1 as proto;

pub fn format_team_insights(resp: &proto::GetTeamInsightsResponse) -> serde_json::Value {
    resp.insights.as_ref().map_or(
        serde_json::json!({"message": "No insights available for this period"}),
        |ins| {
            serde_json::json!({
                "review_quality": ins.review_quality.as_ref().map(|r| serde_json::json!({
                    "avg_depth": r.avg_depth,
                    "total_reviews": r.total_reviews,
                    "rubber_stamp_pct": r.rubber_stamp_pct,
                    "deep_review_pct": r.deep_review_pct,
                    "constructive_count": r.constructive_count,
                    "neutral_count": r.neutral_count,
                    "critical_count": r.critical_count,
                })),
                "pr_significance": ins.pr_significance.as_ref().map(|s| serde_json::json!({
                    "significant_count": s.significant_count,
                    "notable_count": s.notable_count,
                    "routine_count": s.routine_count,
                })),
                "notable_items": ins.notable_items.iter().map(|n| serde_json::json!({
                    "contribution_id": &n.contribution_id,
                    "title": &n.title,
                    "rationale": &n.rationale,
                })).collect::<Vec<_>>(),
                "coverage": ins.coverage.as_ref().map(|c| serde_json::json!({
                    "total_contributions": c.total_contributions,
                    "enriched_count": c.enriched_contributions,
                })),
            })
        },
    )
}

pub fn format_contribution(c: &proto::Contribution) -> serde_json::Value {
    serde_json::json!({
        "id": &c.id,
        "title": &c.title,
        "person_name": &c.person_name,
        "platform": c.platform,
        "contribution_type": c.contribution_type,
        "state": c.state,
        "url": &c.url,
        "repo": &c.repo,
        "created_at": c.created_at.as_ref().map(|t| t.seconds),
    })
}

pub fn format_similar_item(s: &proto::SimilarItem) -> serde_json::Value {
    serde_json::json!({
        "contribution_id": &s.contribution_id,
        "title": &s.title,
        "platform": s.platform,
        "contribution_type": s.contribution_type,
        "author_name": &s.author_name,
        "external_url": &s.external_url,
        "distance": s.distance,
    })
}

pub fn format_person_insights(resp: &proto::GetPersonInsightsResponse) -> serde_json::Value {
    resp.insights.as_ref().map_or(
        serde_json::json!({"message": "No insights available"}),
        |ins| {
            serde_json::json!({
                "reviewer_profile": ins.reviewer_profile.as_ref().map(|r| serde_json::json!({
                    "avg_depth": r.avg_depth,
                    "total_reviews_given": r.total_reviews_given,
                    "rubber_stamp_pct": r.rubber_stamp_pct,
                    "constructive_count": r.constructive_count,
                    "neutral_count": r.neutral_count,
                })),
                "pr_impact": ins.pr_impact.as_ref().map(|s| serde_json::json!({
                    "significant_count": s.significant_count,
                    "notable_count": s.notable_count,
                    "routine_count": s.routine_count,
                })),
                "highlights": ins.highlights.iter().map(|n| serde_json::json!({
                    "contribution_id": &n.contribution_id,
                    "title": &n.title,
                    "rationale": &n.rationale,
                })).collect::<Vec<_>>(),
                "coverage": ins.coverage.as_ref().map(|c| serde_json::json!({
                    "total_contributions": c.total_contributions,
                    "enriched_count": c.enriched_contributions,
                })),
            })
        },
    )
}
