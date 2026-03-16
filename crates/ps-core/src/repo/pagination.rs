use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

/// Parsed page request from the client.
pub struct PageRequest {
    /// Items per page, clamped to 1..=500. 0 means return all (backward compat).
    pub page_size: i64,
    /// Decoded cursor, if present.
    pub cursor: Option<PageCursor>,
}

/// Decoded keyset cursor: last row's sort value + tie-breaker ID.
pub struct PageCursor {
    pub sort_value: String,
    pub id: String,
}

/// Paginated result from a repo method.
pub struct PageResponse<T> {
    pub items: Vec<T>,
    pub next_page_token: Option<String>,
    pub total_count: i64,
}

/// Sort direction.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Validated sort parameters.
pub struct SortParams {
    pub column: String,
    pub direction: SortDir,
}

impl PageRequest {
    /// Build from raw proto values. `page_size` 0 = return all.
    pub fn new(page_size: i32, page_token: &str) -> Self {
        let size = if page_size == 0 {
            0
        } else {
            i64::from(page_size.clamp(1, 500))
        };
        Self {
            page_size: size,
            cursor: decode_cursor(page_token),
        }
    }

    /// SQL LIMIT: `page_size` + 1 to detect next page. None if unpaginated.
    pub fn limit(&self) -> Option<i64> {
        if self.page_size == 0 {
            None
        } else {
            Some(self.page_size + 1)
        }
    }
}

impl SortParams {
    /// Validate a sort field against an allowlist. Returns None if invalid.
    pub fn new(field: &str, descending: bool, allowed: &[&str]) -> Option<Self> {
        if field.is_empty() || !allowed.contains(&field) {
            return None;
        }
        Some(Self {
            column: field.to_owned(),
            direction: if descending {
                SortDir::Desc
            } else {
                SortDir::Asc
            },
        })
    }
}

impl<T> PageResponse<T> {
    /// Build from a vec that may contain one extra peek row.
    pub fn from_items(
        mut items: Vec<T>,
        page_size: i64,
        total_count: i64,
        last_key: impl FnOnce(&T) -> (String, String),
    ) -> Self {
        let len = i64::try_from(items.len()).unwrap_or(i64::MAX);
        if page_size > 0 && len > page_size {
            items.truncate(page_size as usize);
            // We just confirmed len > page_size > 0, so items is non-empty after truncate.
            let Some(last) = items.last() else {
                unreachable!("items non-empty after truncate of non-zero page_size");
            };
            let (sort_val, id) = last_key(last);
            Self {
                items,
                next_page_token: Some(encode_cursor(&sort_val, &id)),
                total_count,
            }
        } else {
            Self {
                items,
                next_page_token: None,
                total_count,
            }
        }
    }
}

fn encode_cursor(sort_value: &str, id: &str) -> String {
    let payload = format!("{sort_value}|{id}");
    URL_SAFE_NO_PAD.encode(payload.as_bytes())
}

fn decode_cursor(token: &str) -> Option<PageCursor> {
    if token.is_empty() {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    let payload = String::from_utf8(bytes).ok()?;
    let (sort_value, id) = payload.split_once('|')?;
    Some(PageCursor {
        sort_value: sort_value.to_owned(),
        id: id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cursor() {
        let encoded = encode_cursor("alice", "abc-123");
        let decoded = decode_cursor(&encoded).unwrap();
        assert_eq!(decoded.sort_value, "alice");
        assert_eq!(decoded.id, "abc-123");
    }

    #[test]
    fn empty_token_returns_none() {
        assert!(decode_cursor("").is_none());
    }

    #[test]
    fn page_request_clamps_size() {
        let req = PageRequest::new(1000, "");
        assert_eq!(req.page_size, 500);
        assert_eq!(req.limit(), Some(501));
    }

    #[test]
    fn page_request_zero_is_unpaginated() {
        let req = PageRequest::new(0, "");
        assert!(req.limit().is_none());
    }

    #[test]
    fn page_response_truncates_extra() {
        let items = vec![1, 2, 3, 4, 5, 6];
        let resp = PageResponse::from_items(items, 5, 10, |i| (i.to_string(), i.to_string()));
        assert_eq!(resp.items.len(), 5);
        assert!(resp.next_page_token.is_some());
        assert_eq!(resp.total_count, 10);
    }

    #[test]
    fn page_response_no_extra() {
        let items = vec![1, 2, 3];
        let resp = PageResponse::from_items(items, 5, 3, |i| (i.to_string(), i.to_string()));
        assert_eq!(resp.items.len(), 3);
        assert!(resp.next_page_token.is_none());
    }
}
