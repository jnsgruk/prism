// ---------------------------------------------------------------------------
// GraphQL query strings
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(super) const FETCH_PRS_QUERY: &str = r"
query($owner: String!, $repo: String!, $cursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequests(
      first: 100
      after: $cursor
      orderBy: { field: UPDATED_AT, direction: ASC }
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number
        title
        state
        url
        isDraft
        createdAt
        updatedAt
        closedAt
        mergedAt
        additions
        deletions
        changedFiles
        author { login }
        bodyText
        labels(first: 10) { nodes { name } }
        headRefName
        baseRefName
        reviews(first: 10) {
          totalCount
          pageInfo { hasNextPage endCursor }
          nodes {
            databaseId
            state
            body
            submittedAt
            author { login }
            comments(first: 20) {
              pageInfo { hasNextPage endCursor }
              nodes { body path }
            }
          }
        }
      }
    }
  }
}
";

pub(super) const SEARCH_PRS_QUERY: &str = r"
query($query: String!, $cursor: String) {
  search(query: $query, type: ISSUE, first: 50, after: $cursor) {
    pageInfo { hasNextPage endCursor }
    issueCount
    nodes {
      ... on PullRequest {
        number
        title
        state
        url
        isDraft
        createdAt
        updatedAt
        closedAt
        mergedAt
        additions
        deletions
        changedFiles
        author { login }
        bodyText
        repository {
          name
          isArchived
          owner { login }
        }
        labels(first: 10) { nodes { name } }
        headRefName
        baseRefName
        reviews(first: 10) {
          totalCount
          pageInfo { hasNextPage endCursor }
          nodes {
            databaseId
            state
            body
            submittedAt
            author { login }
            comments(first: 20) {
              pageInfo { hasNextPage endCursor }
              nodes { body path }
            }
          }
        }
      }
    }
  }
}
";

pub(super) const REVIEWS_QUERY: &str = r"
query($owner: String!, $repo: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviews(first: 100, after: $cursor) {
        totalCount
        pageInfo { hasNextPage endCursor }
        nodes {
          databaseId state body submittedAt author { login }
          comments(first: 20) {
            pageInfo { hasNextPage endCursor }
            nodes { body path }
          }
        }
      }
    }
  }
}
";
