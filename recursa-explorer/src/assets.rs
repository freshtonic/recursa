//! Compile-time-owned Explorer application assets served without filesystem access.

/// One immutable bundled response body and its media type.
pub(super) struct Asset {
    /// HTTP media type emitted for the asset.
    pub(super) content_type: &'static str,
    /// Bytes embedded in the crate package at compile time.
    pub(super) body: &'static [u8],
}

/// Resolves the complete dependency-free application by exact request path.
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
        "/api.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/api.js"),
        }),
        "/offsets.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/offsets.js"),
        }),
        "/model.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/model.js"),
        }),
        "/tree.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/tree.js"),
        }),
        "/views.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/views.js"),
        }),
        "/panzoom.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/panzoom.js"),
        }),
        "/graph.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/graph.js"),
        }),
        "/grain.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/grain.js"),
        }),
        "/strata.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/strata.js"),
        }),
        "/railroad-path.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../assets/railroad-path.js"),
        }),
        "/fonts/inter-variable.woff2" => Some(Asset {
            content_type: "font/woff2",
            body: include_bytes!("../assets/fonts/inter-variable.woff2"),
        }),
        "/fonts/ibmplexmono-400.woff2" => Some(Asset {
            content_type: "font/woff2",
            body: include_bytes!("../assets/fonts/ibmplexmono-400.woff2"),
        }),
        "/fonts/ibmplexmono-500.woff2" => Some(Asset {
            content_type: "font/woff2",
            body: include_bytes!("../assets/fonts/ibmplexmono-500.woff2"),
        }),
        "/fonts/ibmplexmono-600.woff2" => Some(Asset {
            content_type: "font/woff2",
            body: include_bytes!("../assets/fonts/ibmplexmono-600.woff2"),
        }),
        _ => None,
    }
}
