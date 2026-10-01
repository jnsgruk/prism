//! Validate generated file links while preserving unrelated Markdown verbatim.
use super::workspace_files::{resolve_file, valid_relative_path};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::{collections::HashMap, path::PathBuf};
use uuid::Uuid;

pub(crate) fn reference_path(href: &str, conversation: Uuid) -> Option<Result<String, ()>> {
    let canonical = format!("/ask/{conversation}/files/");
    let encoded = if let Some(path) = href.strip_prefix("/workspace/") {
        path
    } else if let Some(path) = href.strip_prefix("workspace/") {
        path
    } else if let Some(path) = href.strip_prefix(&canonical) {
        path
    } else if href.starts_with("/ask/") && href.contains("/files/") {
        return Some(Err(()));
    } else {
        if href.contains(':')
            || href.starts_with('/')
            || href.starts_with('#')
            || href.contains(['?', '#'])
        {
            return None;
        }
        let decoded = decode_segments(href.strip_prefix("./").unwrap_or(href));
        let path = decoded.as_ref().ok().map_or(href, String::as_str);
        let extension = path.rsplit('.').next()?.to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "pdf"
                | "csv"
                | "tsv"
                | "txt"
                | "md"
                | "json"
                | "zip"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "htm"
                | "html"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "bmp"
        ) {
            return None;
        }
        return Some(decoded);
    };
    Some(decode_segments(encoded))
}

fn decode_segments(encoded: &str) -> Result<String, ()> {
    let mut segments = Vec::new();
    for segment in encoded.split('/') {
        let mut bytes = Vec::new();
        let raw = segment.as_bytes();
        let mut i = 0;
        while let Some(&byte) = raw.get(i) {
            if byte == b'%' {
                let pair = raw.get(i + 1..i + 3).ok_or(())?;
                let hex = std::str::from_utf8(pair).map_err(|_| ())?;
                bytes.push(u8::from_str_radix(hex, 16).map_err(|_| ())?);
                i += 3;
            } else {
                bytes.push(byte);
                i += 1;
            }
        }
        let decoded = String::from_utf8(bytes).map_err(|_| ())?;
        if decoded.contains('/') || !valid_relative_path(&decoded) {
            return Err(());
        }
        segments.push(decoded);
    }
    let path = segments.join("/");
    valid_relative_path(&path).then_some(path).ok_or(())
}

pub(crate) fn download_url(conversation: Uuid, path: &str) -> String {
    let encoded = path
        .split('/')
        .map(|segment| {
            segment
                .as_bytes()
                .iter()
                .map(|byte| {
                    if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(byte) {
                        char::from(*byte).to_string()
                    } else {
                        format!("%{byte:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("/ask/{conversation}/files/{encoded}")
}

fn link_label(source: &str) -> Option<&str> {
    let start = source.find('[')? + 1;
    let mut depth = 1;
    let mut escaped = false;
    for (offset, ch) in source[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[start..start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_sync(answer: &str, root: Option<&std::path::Path>, conversation: Uuid) -> String {
    let mut replacements = Vec::new();
    let mut resolved = HashMap::new();
    let mut unavailable = false;
    let mut image_depth = 0;
    for (event, range) in Parser::new(answer).into_offset_iter() {
        let dest_url = match event {
            Event::Start(Tag::Image { .. }) => {
                image_depth += 1;
                continue;
            }
            Event::End(TagEnd::Image) => {
                image_depth -= 1;
                continue;
            }
            Event::Start(Tag::Link { dest_url, .. }) if image_depth == 0 => dest_url,
            _ => continue,
        };
        let Some(path) = reference_path(&dest_url, conversation) else {
            continue;
        };
        let Some(label) = link_label(&answer[range.clone()]) else {
            continue;
        };
        let url = path.ok().and_then(|path| {
            if resolved.len() >= 128 && !resolved.contains_key(&path) {
                return None;
            }
            resolved
                .entry(path.clone())
                .or_insert_with(|| {
                    root.and_then(|root| resolve_file(root, &conversation.to_string(), &path).ok())
                        .map(|_| download_url(conversation, &path))
                })
                .clone()
        });
        let replacement = if let Some(url) = url {
            format!("[{label}](<{url}>)")
        } else {
            unavailable = true;
            format!("{label} (file unavailable)")
        };
        replacements.push((range, replacement));
    }
    let mut result = answer.to_string();
    for (range, replacement) in replacements.into_iter().rev() {
        result.replace_range(range, &replacement);
    }
    if unavailable {
        result.push_str(
            "\n\nSome referenced files could not be verified and are unavailable for download.",
        );
    }
    result
}

pub(crate) async fn validate_file_links(
    answer: String,
    root: Option<PathBuf>,
    conversation: Uuid,
) -> String {
    let fallback = answer.clone();
    tokio::task::spawn_blocking(move || validate_sync(&answer, root.as_deref(), conversation))
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "answer file validation failed");
            validate_sync(&fallback, None, conversation)
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn shared_path_encoding_cases() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../fixtures/workspace-paths.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let actual = decode_segments(case["encoded"].as_str().unwrap()).ok();
            let expected = case["path"].as_str().map(str::to_string);
            assert_eq!(actual, expected, "{case}");
        }
        let id = Uuid::new_v4();
        assert!(reference_path("../guide", id).is_none());
        assert!(reference_path("report%2Epdf", id).unwrap().is_ok());
        assert!(
            reference_path(&format!("/ask/{}/files/report.pdf", Uuid::new_v4()), id)
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn validates_links_without_touching_code_images_or_external_urls() {
        let temp = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        std::fs::create_dir(temp.path().join(id.to_string())).unwrap();
        std::fs::write(temp.path().join(id.to_string()).join("a %.pdf"), b"PDF").unwrap();
        let raw = "Ready: [**report**](/workspace/a%20%25.pdf)\n[missing][m]\n\n[m]: /workspace/missing.pdf\n\n`[code](/workspace/no.pdf)`\n![image](/workspace/no.png)\n[web](https://example.org/a.pdf)";
        let validated = validate_sync(raw, Some(temp.path()), id);
        assert!(validated.contains(&format!("[**report**](</ask/{id}/files/a%20%25.pdf>)")));
        assert!(validated.contains("missing (file unavailable)"));
        assert!(validated.contains("`[code](/workspace/no.pdf)`"));
        assert!(validated.contains("![image](/workspace/no.png)"));
        assert!(validated.contains("[web](https://example.org/a.pdf)"));

        let image = "![alt [file](/workspace/missing.pdf)](/workspace/image.png)";
        assert_eq!(validate_sync(image, Some(temp.path()), id), image);
        let image = "![alt [file](/workspace/a%20%25.pdf)](/workspace/image.png)";
        assert_eq!(validate_sync(image, Some(temp.path()), id), image);
    }
}
