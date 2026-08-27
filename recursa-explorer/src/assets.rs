//! Compile-time-owned placeholder assets served without filesystem access.

/// One immutable bundled response body and its media type.
pub(super) struct Asset {
    /// HTTP media type emitted for the asset.
    pub(super) content_type: &'static str,
    /// Bytes embedded in the crate package at compile time.
    pub(super) body: &'static [u8],
}

/// Resolves the small placeholder asset set by exact request path.
pub(super) fn get(path: &str) -> Option<Asset> {
    match path {
        "/" | "/index.html" => Some(Asset {
            content_type: "text/html; charset=utf-8",
            body: include_bytes!("../assets/index.html"),
        }),
        "/style.css" => Some(Asset {
            content_type: "text/css; charset=utf-8",
            body: include_bytes!("../assets/style.css"),
        }),
        "/app.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/app.js"),
        }),
        _ => None,
    }
}
