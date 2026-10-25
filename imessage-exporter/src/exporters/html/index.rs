//! Root index for HTML exports.

use std::{collections::HashMap, io::Write, path::Path};

use crate::{
    app::{error::RuntimeError, sanitizers::sanitize_html},
    exporters::shared::driver::ConversationSummary,
};

const INDEX_FILENAME: &str = "index.html";

/// Write a self-contained conversation browser after all transcript files have
/// been flushed. The page has no script and no network-capable content.
pub(super) fn write_index(
    export_path: &Path,
    conversations: &HashMap<String, ConversationSummary>,
) -> Result<(), RuntimeError> {
    let mut entries: Vec<_> = conversations.iter().collect();
    entries.sort_by(|(left_name, left), (right_name, right)| {
        right
            .latest
            .as_ref()
            .map(|latest| latest.at)
            .cmp(&left.latest.as_ref().map(|latest| latest.at))
            .then_with(|| left_name.cmp(right_name))
    });

    let mut page = String::from(
        "<!doctype html><html><head><meta charset=\"UTF-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>iMessage Archive</title><style>body{font-family:system-ui,-apple-system,sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;color:#1d1d1f}h1{margin-bottom:.25rem}.summary{color:#666;margin-top:0}.chat{display:block;padding:1rem;border:1px solid #ddd;border-radius:.6rem;margin:.75rem 0;color:inherit;text-decoration:none}.chat:hover{background:#f5f5f7}.name{font-weight:650}.meta{color:#666;margin-top:.25rem;font-size:.9rem}</style></head><body><h1>iMessage Archive</h1><p class=\"summary\">",
    );
    page.push_str(&format!(
        "{} conversation{}",
        entries.len(),
        plural(entries.len())
    ));
    page.push_str("</p>");

    for (filename, summary) in entries {
        let label = filename.strip_suffix(".html").unwrap_or(filename);
        page.push_str("<a class=\"chat\" href=\"");
        page.push_str(&url_path(filename));
        page.push_str("\"><div class=\"name\">");
        page.push_str(&sanitize_html(label));
        page.push_str("</div><div class=\"meta\">");
        page.push_str(&format!(
            "{} message{}",
            summary.message_count,
            plural(summary.message_count as usize)
        ));
        if let Some(latest) = &summary.latest {
            page.push_str(" · Latest: ");
            page.push_str(&sanitize_html(&latest.formatted));
        }
        page.push_str("</div></a>");
    }
    page.push_str("</body></html>\n");

    let mut temp = tempfile::NamedTempFile::new_in(export_path)?;
    temp.write_all(page.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(export_path.join(INDEX_FILENAME))
        .map_err(|error| RuntimeError::DiskError(error.error))?;
    Ok(())
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Percent-encode filenames so punctuation and Unicode remain local links.
fn url_path(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use super::write_index;
    use crate::exporters::shared::driver::ConversationSummary;

    #[test]
    fn index_escapes_labels_and_percent_encodes_links() {
        let temp = tempfile::tempdir().unwrap();
        let mut conversations = HashMap::new();
        conversations.insert(
            "A & B.html".to_string(),
            ConversationSummary {
                message_count: 2,
                latest: None,
            },
        );

        write_index(temp.path(), &conversations).unwrap();
        let page = fs::read_to_string(temp.path().join("index.html")).unwrap();
        assert!(page.contains("href=\"A%20%26%20B.html\""));
        assert!(page.contains("A &amp; B"));
        assert!(page.contains("2 messages"));
    }
}
