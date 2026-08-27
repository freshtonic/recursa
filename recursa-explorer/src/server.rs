//! Loopback HTTP lifecycle behind the crate's single public launch function.

use crate::{RunError, assets, browser, protocol};
use recursa::explorer::Adapter;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::io::{self, Read};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

/// Largest accepted JSON request body, preventing loopback memory exhaustion.
const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// CSP for the dependency-free same-origin asset and protocol surface.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Binds the listener before opening the system browser and serves until shutdown.
pub(super) fn run(adapter: Adapter) -> Result<(), RunError> {
    run_with(adapter, browser::open)
}

/// Internal browser seam used to prove readiness and cleanup without opening a real browser.
fn run_with<O, S>(adapter: Adapter, opener: O) -> Result<(), RunError>
where
    O: FnOnce(&str) -> io::Result<S>,
{
    let server = Server::http(("127.0.0.1", 0)).map_err(|error| RunError::boxed("bind", error))?;
    let address = server
        .server_addr()
        .to_ip()
        .filter(|address| address.ip().is_loopback())
        .ok_or_else(|| RunError::new("bind", io::Error::other("listener is not loopback IP")))?;
    let authority = address.to_string();
    let origin = format!("http://{authority}");
    let url = format!("{origin}/");
    let opener_session = opener(&url).map_err(|error| RunError::new("browser launch", error))?;

    let result = serve(&server, adapter, &authority, &origin);
    drop(opener_session);
    result
}

/// Processes one request at a time so shutdown is ordered after its response is written.
fn serve(server: &Server, adapter: Adapter, authority: &str, origin: &str) -> Result<(), RunError> {
    loop {
        let request = server
            .recv()
            .map_err(|error| RunError::new("request receive", error))?;
        let outcome = handle(request, adapter, authority, origin)
            .map_err(|error| RunError::new("response", error))?;
        if outcome == Outcome::Shutdown {
            return Ok(());
        }
    }
}

/// Whether a completed response ends the server lifecycle.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Outcome {
    /// Continue accepting requests.
    Continue,
    /// Stop after the current response is fully handed off.
    Shutdown,
}

/// Validates same-origin policy, dispatches one stateless operation, and responds.
fn handle(
    mut request: Request,
    adapter: Adapter,
    authority: &str,
    origin: &str,
) -> io::Result<Outcome> {
    if !request
        .remote_addr()
        .is_some_and(|address| address.ip().is_loopback())
    {
        respond_json(
            request,
            403,
            json_error("forbidden", "loopback clients only"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }
    if header(&request, "Host") != Some(authority) {
        respond_json(
            request,
            403,
            json_error("forbidden", "invalid Host header"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }
    let request_origin = header(&request, "Origin");
    if request_origin.is_some_and(|value| value != origin)
        || (*request.method() == Method::Post && request_origin != Some(origin))
    {
        respond_json(
            request,
            403,
            json_error("forbidden", "invalid Origin header"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }

    let path = request.url().to_owned();
    if let Some(asset) = assets::get(&path) {
        if request.method() != &Method::Get {
            respond_json(
                request,
                405,
                json_error("method", "GET required"),
                Some("GET"),
            )?;
        } else {
            respond_bytes(request, 200, asset.content_type, asset.body.to_vec(), None)?;
        }
        return Ok(Outcome::Continue);
    }
    let expected_method = match path.as_str() {
        "/api/schema" => Some(Method::Get),
        "/api/lex" | "/api/parse" | "/api/pretty" | "/api/sample" | "/api/shutdown" => {
            Some(Method::Post)
        }
        _ => None,
    };
    let Some(expected_method) = expected_method else {
        respond_json(request, 404, json_error("not_found", "unknown path"), None)?;
        return Ok(Outcome::Continue);
    };
    if request.method() != &expected_method {
        let allow = if expected_method == Method::Get {
            "GET"
        } else {
            "POST"
        };
        respond_json(
            request,
            405,
            json_error("method", "method not allowed"),
            Some(allow),
        )?;
        return Ok(Outcome::Continue);
    }

    if expected_method == Method::Get {
        respond_json(request, 200, protocol::schema(adapter), None)?;
        return Ok(Outcome::Continue);
    }
    if !is_json_content_type(&request) {
        respond_json(
            request,
            415,
            json_error("content_type", "application/json required"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }
    let Some(length) = request.body_length() else {
        respond_json(
            request,
            411,
            json_error("length", "Content-Length required"),
            None,
        )?;
        return Ok(Outcome::Continue);
    };
    if length > MAX_REQUEST_BYTES {
        respond_json(
            request,
            413,
            json_error("size", "request body too large"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }
    let mut body = Vec::with_capacity(length);
    request
        .as_reader()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut body)?;
    if body.len() > MAX_REQUEST_BYTES {
        respond_json(
            request,
            413,
            json_error("size", "request body too large"),
            None,
        )?;
        return Ok(Outcome::Continue);
    }

    let value = match path.as_str() {
        "/api/lex" => parse_body::<protocol::SourceRequest>(&body)
            .map(|request| protocol::lex(adapter, &request.source)),
        "/api/parse" => parse_body::<protocol::SourceRequest>(&body)
            .map(|request| protocol::parse(adapter, &request.source)),
        "/api/pretty" => parse_body::<protocol::PrettyRequest>(&body)
            .map(|request| protocol::pretty(adapter, &request)),
        "/api/sample" => parse_body::<protocol::SampleRequest>(&body)
            .map(|request| protocol::sample(adapter, &request.bytes)),
        "/api/shutdown" => {
            parse_body::<serde_json::Map<String, Value>>(&body).and_then(|request| {
                request
                    .is_empty()
                    .then_some(json!({ "status": "ok" }))
                    .ok_or_else(|| "shutdown body must be an empty object".to_owned())
            })
        }
        _ => unreachable!("known POST routes are exhaustive"),
    };
    match value {
        Ok(value) => {
            respond_json(request, 200, value, None)?;
            Ok(if path == "/api/shutdown" {
                Outcome::Shutdown
            } else {
                Outcome::Continue
            })
        }
        Err(message) => {
            respond_json(request, 400, json_error("json", &message), None)?;
            Ok(Outcome::Continue)
        }
    }
}

/// Deserializes one closed request DTO with human-readable error evidence.
fn parse_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|error| error.to_string())
}

/// Reports whether a request declares the one accepted JSON media type.
fn is_json_content_type(request: &Request) -> bool {
    header(request, "Content-Type").is_some_and(|value| {
        let mut parts = value.split(';').map(str::trim);
        parts.next() == Some("application/json")
            && parts.all(|parameter| parameter.eq_ignore_ascii_case("charset=utf-8"))
    })
}

/// Returns an exact header value without accepting duplicates.
fn header<'request>(request: &'request Request, name: &'static str) -> Option<&'request str> {
    let mut values = request
        .headers()
        .iter()
        .filter(|header| header.field.equiv(name))
        .map(|header| header.value.as_str());
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

/// Creates the uniform private JSON error envelope.
fn json_error(code: &str, message: &str) -> Value {
    json!({ "error": { "code": code, "message": message } })
}

/// Serializes one JSON response and attaches the common security headers.
fn respond_json(
    request: Request,
    status: u16,
    value: Value,
    allow: Option<&str>,
) -> io::Result<()> {
    let body = serde_json::to_vec(&value).expect("JSON Value serialization is infallible");
    respond_bytes(
        request,
        status,
        "application/json; charset=utf-8",
        body,
        allow,
    )
}

/// Emits one bounded in-memory response with security and cache policy.
fn respond_bytes(
    request: Request,
    status: u16,
    content_type: &str,
    body: Vec<u8>,
    allow: Option<&str>,
) -> io::Result<()> {
    let mut response = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(http_header("Content-Type", content_type))
        .with_header(http_header(
            "Content-Security-Policy",
            CONTENT_SECURITY_POLICY,
        ))
        .with_header(http_header("X-Content-Type-Options", "nosniff"))
        .with_header(http_header("Referrer-Policy", "no-referrer"))
        .with_header(http_header(
            "Permissions-Policy",
            "camera=(), microphone=(), geolocation=()",
        ))
        .with_header(http_header("Cache-Control", "no-store"));
    if let Some(allow) = allow {
        response.add_header(http_header("Allow", allow));
    }
    request.respond(response)
}

/// Builds a header from validated static names and controlled values.
fn http_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid server header")
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn server_is_ready_before_opening_and_accepts_orderly_shutdown() {
        let adapter = recursa_example_json::grammar::Value::explorer();
        let (url_sender, url_receiver) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_in_thread = Arc::clone(&dropped);
        let handle = thread::spawn(move || {
            super::run_with(adapter, move |url| {
                let authority = url.strip_prefix("http://").unwrap().trim_end_matches('/');
                TcpStream::connect(authority).expect("listener is ready before opener");
                url_sender.send(url.to_owned()).unwrap();
                Ok(DropProbe(dropped_in_thread))
            })
        });
        let url = url_receiver.recv().unwrap();
        let authority = url.strip_prefix("http://").unwrap().trim_end_matches('/');

        let schema = request(authority, "GET", "/api/schema", None, None);
        assert!(schema.starts_with("HTTP/1.1 200"), "{schema}");
        assert!(schema.contains("\"name\":\"grammar\""), "{schema}");

        let shutdown = request(
            authority,
            "POST",
            "/api/shutdown",
            Some("{}"),
            Some(&format!("http://{authority}")),
        );
        assert!(shutdown.starts_with("HTTP/1.1 200"), "{shutdown}");
        handle.join().unwrap().unwrap();
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn browser_launch_failure_reports_the_failed_operation() {
        let error = super::run_with(recursa_example_json::grammar::Value::explorer(), |_| {
            Err::<(), _>(std::io::Error::other("test opener unavailable"))
        })
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Explorer browser launch failed: test opener unavailable"
        );
    }

    #[test]
    fn server_enforces_same_origin_methods_content_type_and_request_size() {
        let (authority, handle, _) = spawn_server();
        let origin = format!("http://{authority}");

        let wrong_host = raw_request(
            &authority,
            "GET /api/schema HTTP/1.1\r\nHost: attacker.invalid\r\nConnection: close\r\n\r\n",
        );
        assert!(wrong_host.starts_with("HTTP/1.1 403"), "{wrong_host}");

        let wrong_origin = raw_request(
            &authority,
            &format!(
                "POST /api/parse HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://attacker.invalid\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            ),
        );
        assert!(wrong_origin.starts_with("HTTP/1.1 403"), "{wrong_origin}");
        assert!(!wrong_origin.contains("Access-Control-Allow-Origin"));

        let missing_origin = raw_request(
            &authority,
            &format!(
                "POST /api/parse HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            ),
        );
        assert!(
            missing_origin.starts_with("HTTP/1.1 403"),
            "{missing_origin}"
        );

        let duplicate_host = raw_request(
            &authority,
            &format!(
                "GET /api/schema HTTP/1.1\r\nHost: {authority}\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(
            duplicate_host.starts_with("HTTP/1.1 403"),
            "{duplicate_host}"
        );

        let wrong_method = request(&authority, "POST", "/api/schema", Some("{}"), Some(&origin));
        assert!(wrong_method.starts_with("HTTP/1.1 405"), "{wrong_method}");
        assert!(wrong_method.contains("Allow: GET"), "{wrong_method}");

        let wrong_type = raw_request(
            &authority,
            &format!(
                "POST /api/parse HTTP/1.1\r\nHost: {authority}\r\nOrigin: {origin}\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            ),
        );
        assert!(wrong_type.starts_with("HTTP/1.1 415"), "{wrong_type}");

        let too_large = raw_request(
            &authority,
            &format!(
                "POST /api/parse HTTP/1.1\r\nHost: {authority}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                super::MAX_REQUEST_BYTES + 1
            ),
        );
        assert!(too_large.starts_with("HTTP/1.1 413"), "{too_large}");

        for (body, message) in [
            (
                r#"{"source":"null","max_width":0,"indent":2}"#,
                "max_width must be between 1 and 1000",
            ),
            (
                r#"{"source":"null","max_width":1001,"indent":2}"#,
                "max_width must be between 1 and 1000",
            ),
            (
                r#"{"source":"null","max_width":80,"indent":33}"#,
                "indent must be between 0 and 32",
            ),
        ] {
            let response = request(&authority, "POST", "/api/pretty", Some(body), Some(&origin));
            assert!(response.starts_with("HTTP/1.1 400"), "{response}");
            assert!(response.contains(r#""code":"json""#), "{response}");
            assert!(response.contains(message), "{response}");
            assert!(
                !response.contains(r#""status":"unsupported""#),
                "{response}"
            );
        }

        shutdown(&authority);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn assets_and_protocol_are_private_stateless_and_structured() {
        let (authority, handle, _) = spawn_server();
        let origin = format!("http://{authority}");

        let index = request(&authority, "GET", "/", None, None);
        assert!(index.starts_with("HTTP/1.1 200"), "{index}");
        assert!(
            index.contains("Content-Security-Policy: default-src 'self'"),
            "{index}"
        );
        assert!(index.contains("X-Content-Type-Options: nosniff"), "{index}");
        assert!(index.contains("form-action 'none'"), "{index}");
        assert!(index.contains("Referrer-Policy: no-referrer"), "{index}");
        assert!(
            index.contains("Permissions-Policy: camera=(), microphone=(), geolocation=()"),
            "{index}"
        );
        assert!(index.contains("aria-label=\"Explorer AST\""), "{index}");
        assert!(index.contains("type=\"module\""), "{index}");

        for (path, marker) in [
            ("/app.js", "AbortController"),
            ("/api.js", "credentials: \"same-origin\""),
            ("/offsets.js", "TextEncoder"),
            ("/model.js", "occurrenceEntries"),
            ("/tree.js", "role = \"treeitem\""),
            ("/views.js", "textContent"),
        ] {
            let script = request(&authority, "GET", path, None, None);
            assert!(script.starts_with("HTTP/1.1 200"), "{path}: {script}");
            assert!(
                script.contains("Content-Type: text/javascript; charset=utf-8"),
                "{path}: {script}"
            );
            assert!(script.contains(marker), "{path}: {script}");
            assert!(!script.contains("innerHTML"), "{path}: {script}");
        }

        let styles = request(&authority, "GET", "/style.css", None, None);
        assert!(styles.starts_with("HTTP/1.1 200"), "{styles}");
        assert!(styles.contains("prefers-reduced-motion"), "{styles}");

        let missing = request(&authority, "GET", "/../Cargo.toml", None, None);
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");

        let lexed = request(
            &authority,
            "POST",
            "/api/lex",
            Some(r#"{"source":"null @"}"#),
            Some(&origin),
        );
        assert!(lexed.starts_with("HTTP/1.1 200"), "{lexed}");
        assert!(lexed.contains(r#""text":"null""#), "{lexed}");
        assert!(lexed.contains(r#""text":"@""#), "{lexed}");

        let parsed = request(
            &authority,
            "POST",
            "/api/parse",
            Some(r#"{"source":"[true,null]"}"#),
            Some(&origin),
        );
        assert!(parsed.contains(r#""status":"ok""#), "{parsed}");
        assert!(parsed.contains(r#""activeVariant":"Array""#), "{parsed}");

        let failed = request(
            &authority,
            "POST",
            "/api/parse",
            Some(r#"{"source":"["}"#),
            Some(&origin),
        );
        assert!(failed.contains(r#""status":"error""#), "{failed}");
        assert!(failed.contains(r#""diagnostics":[{"#), "{failed}");
        assert!(!failed.contains(r#""document""#), "{failed}");

        let pretty = request(
            &authority,
            "POST",
            "/api/pretty",
            Some(r#"{"source":"null"}"#),
            Some(&origin),
        );
        assert!(pretty.contains(r#""status":"unsupported""#), "{pretty}");
        let sample = request(
            &authority,
            "POST",
            "/api/sample",
            Some(r#"{"bytes":[1,2,3]}"#),
            Some(&origin),
        );
        assert!(sample.contains(r#""status":"unsupported""#), "{sample}");

        let malformed = request(&authority, "POST", "/api/parse", Some("{"), Some(&origin));
        assert!(malformed.starts_with("HTTP/1.1 400"), "{malformed}");
        assert!(malformed.contains(r#""code":"json""#), "{malformed}");

        shutdown(&authority);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn pretty_and_sample_capabilities_round_trip_deterministically() {
        let (authority, handle, _) =
            spawn_with_adapter(recursa_explorer_capabilities_fixture::grammar::Root::explorer());
        let origin = format!("http://{authority}");

        let pretty = request(
            &authority,
            "POST",
            "/api/pretty",
            Some(r#"{"source":"ready","max_width":80,"indent":2}"#),
            Some(&origin),
        );
        assert!(pretty.contains(r#""status":"ok""#), "{pretty}");
        assert!(pretty.contains(r#""rendered":"ready""#), "{pretty}");
        assert!(pretty.contains(r#""sourceMap":[{"#), "{pretty}");

        for body in [
            r#"{"source":"ready","max_width":1,"indent":0}"#,
            r#"{"source":"ready","max_width":1000,"indent":32}"#,
        ] {
            let boundary = request(&authority, "POST", "/api/pretty", Some(body), Some(&origin));
            assert!(boundary.starts_with("HTTP/1.1 200"), "{boundary}");
            assert!(boundary.contains(r#""status":"ok""#), "{boundary}");
        }

        let first = request(
            &authority,
            "POST",
            "/api/sample",
            Some(r#"{"bytes":[7,11,13]}"#),
            Some(&origin),
        );
        let repeated = request(
            &authority,
            "POST",
            "/api/sample",
            Some(r#"{"bytes":[7,11,13]}"#),
            Some(&origin),
        );
        assert!(first.contains(r#""status":"ok""#), "{first}");
        assert!(first.contains(r#""provenance":"Synthetic""#), "{first}");
        assert_eq!(response_body(&first), response_body(&repeated));

        shutdown(&authority);
        handle.join().unwrap().unwrap();
    }

    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn spawn_server() -> (
        String,
        thread::JoinHandle<Result<(), crate::RunError>>,
        Arc<AtomicBool>,
    ) {
        spawn_with_adapter(recursa_example_json::grammar::Value::explorer())
    }

    fn spawn_with_adapter(
        adapter: recursa::explorer::Adapter,
    ) -> (
        String,
        thread::JoinHandle<Result<(), crate::RunError>>,
        Arc<AtomicBool>,
    ) {
        let (url_sender, url_receiver) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_in_thread = Arc::clone(&dropped);
        let handle = thread::spawn(move || {
            super::run_with(adapter, move |url| {
                url_sender.send(url.to_owned()).unwrap();
                Ok(DropProbe(dropped_in_thread))
            })
        });
        let url = url_receiver.recv().unwrap();
        (
            url.strip_prefix("http://")
                .unwrap()
                .trim_end_matches('/')
                .to_owned(),
            handle,
            dropped,
        )
    }

    fn shutdown(authority: &str) {
        let response = request(
            authority,
            "POST",
            "/api/shutdown",
            Some("{}"),
            Some(&format!("http://{authority}")),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    fn request(
        authority: &str,
        method: &str,
        path: &str,
        body: Option<&str>,
        origin: Option<&str>,
    ) -> String {
        let body = body.unwrap_or("");
        let mut stream = TcpStream::connect(authority).unwrap();
        let origin = origin.map_or_else(String::new, |value| format!("Origin: {value}\r\n"));
        let content_type = if body.is_empty() {
            ""
        } else {
            "Content-Type: application/json\r\n"
        };
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {authority}\r\n{origin}{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn raw_request(authority: &str, request: &str) -> String {
        let mut stream = TcpStream::connect(authority).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn response_body(response: &str) -> &str {
        response.split_once("\r\n\r\n").unwrap().1
    }
}
