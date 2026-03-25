use ps_proto::canonical::prism::v1 as proto;

pub fn parse_insight_period(s: &str) -> i32 {
    match s.to_lowercase().as_str() {
        "last_week" | "week" => proto::InsightPeriod::LastWeek.into(),
        "last_quarter" | "quarter" => proto::InsightPeriod::LastQuarter.into(),
        "last_year" | "year" => proto::InsightPeriod::LastYear.into(),
        // Default to last_month for unrecognised periods.
        _ => proto::InsightPeriod::LastMonth.into(),
    }
}

pub fn platform_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::Platform::Unspecified.into(), |v| {
        proto::Platform::from_user_str(v).into()
    })
}

pub fn contribution_type_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::ContributionType::Unspecified.into(), |v| {
        proto::ContributionType::from_user_str(v).into()
    })
}

pub fn state_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::ContributionState::Unspecified.into(), |v| {
        proto::ContributionState::from_user_str(v).into()
    })
}

pub fn guess_content_type(filename: &str) -> &'static str {
    match filename.rsplit('.').next() {
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("html") => "text/html",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Enum conversions (delegating to ps_proto::convert)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_insight_period_valid() {
        assert_eq!(
            parse_insight_period("last_week"),
            i32::from(proto::InsightPeriod::LastWeek)
        );
        assert_eq!(
            parse_insight_period("week"),
            i32::from(proto::InsightPeriod::LastWeek)
        );
        assert_eq!(
            parse_insight_period("LAST_QUARTER"),
            i32::from(proto::InsightPeriod::LastQuarter)
        );
    }

    #[test]
    fn parse_insight_period_defaults_to_month() {
        assert_eq!(
            parse_insight_period("unknown"),
            i32::from(proto::InsightPeriod::LastMonth)
        );
        assert_eq!(
            parse_insight_period(""),
            i32::from(proto::InsightPeriod::LastMonth)
        );
    }

    #[test]
    fn platform_str_to_proto_values() {
        assert_eq!(
            platform_str_to_proto(Some("github")),
            i32::from(proto::Platform::Github)
        );
        assert_eq!(
            platform_str_to_proto(Some("GitHub")),
            i32::from(proto::Platform::Github)
        );
        assert_eq!(
            platform_str_to_proto(Some("JIRA")),
            i32::from(proto::Platform::Jira)
        );
        assert_eq!(
            platform_str_to_proto(None),
            i32::from(proto::Platform::Unspecified)
        );
    }

    #[test]
    fn contribution_type_str_to_proto_all_variants() {
        assert_eq!(
            contribution_type_str_to_proto(Some("pull_request")),
            i32::from(proto::ContributionType::PullRequest)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("pr_review")),
            i32::from(proto::ContributionType::PrReview)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("review")),
            i32::from(proto::ContributionType::PrReview)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("discourse_like")),
            i32::from(proto::ContributionType::DiscourseLike)
        );
        assert_eq!(
            contribution_type_str_to_proto(None),
            i32::from(proto::ContributionType::Unspecified)
        );
    }

    #[test]
    fn state_str_to_proto_all_variants() {
        assert_eq!(
            state_str_to_proto(Some("open")),
            i32::from(proto::ContributionState::Open)
        );
        assert_eq!(
            state_str_to_proto(Some("MERGED")),
            i32::from(proto::ContributionState::Merged)
        );
        assert_eq!(
            state_str_to_proto(Some("in_progress")),
            i32::from(proto::ContributionState::InProgress)
        );
        assert_eq!(
            state_str_to_proto(Some("approved")),
            i32::from(proto::ContributionState::Approved)
        );
        assert_eq!(
            state_str_to_proto(Some("done")),
            i32::from(proto::ContributionState::Done)
        );
        assert_eq!(
            state_str_to_proto(None),
            i32::from(proto::ContributionState::Unspecified)
        );
    }

    // -----------------------------------------------------------------------
    // Content type guessing
    // -----------------------------------------------------------------------

    #[test]
    fn guess_content_type_known_extensions() {
        assert_eq!(guess_content_type("report.csv"), "text/csv");
        assert_eq!(guess_content_type("data.json"), "application/json");
        assert_eq!(guess_content_type("readme.md"), "text/markdown");
        assert_eq!(guess_content_type("notes.txt"), "text/plain");
        assert_eq!(guess_content_type("chart.png"), "image/png");
        assert_eq!(guess_content_type("doc.pdf"), "application/pdf");
    }

    #[test]
    fn guess_content_type_unknown_defaults_to_octet_stream() {
        assert_eq!(
            guess_content_type("archive.tar.gz"),
            "application/octet-stream"
        );
        assert_eq!(guess_content_type("noext"), "application/octet-stream");
    }
}
