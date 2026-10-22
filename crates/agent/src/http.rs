pub const MAX_HEAD_LEN: usize = 8192;
const MAX_HEADERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    pub host: Option<String>,
    pub origin: Option<String>,
    pub wants_websocket: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadError {
    Malformed,
    DuplicateHost,
}

pub fn parse_head(bytes: &[u8]) -> Result<Option<RequestHead>, HeadError> {
    let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut slots);
    match request.parse(bytes) {
        Ok(httparse::Status::Partial) => return Ok(None),
        Ok(httparse::Status::Complete(_)) => {}
        Err(_) => return Err(HeadError::Malformed),
    }
    let (Some(method), Some(path)) = (request.method, request.path) else {
        return Err(HeadError::Malformed);
    };
    let mut hosts = request
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("host"));
    let host = hosts.next().map(|h| value(h.value));
    if hosts.next().is_some() {
        return Err(HeadError::DuplicateHost);
    }
    let origin = find(request.headers, "origin");
    let wants_websocket = find(request.headers, "upgrade")
        .is_some_and(|v| v.to_ascii_lowercase().contains("websocket"));
    Ok(Some(RequestHead {
        method: method.to_owned(),
        path: path.to_owned(),
        host,
        origin,
        wants_websocket,
    }))
}

pub fn allowed_authorities(port: u16) -> [String; 3] {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
}

pub fn host_allowed(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|host| {
        allowed_authorities(port)
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
    })
}

pub fn origin_allowed(origin: Option<&str>, port: u16) -> bool {
    origin.is_none_or(|origin| {
        allowed_authorities(port)
            .iter()
            .any(|allowed| format!("http://{allowed}").eq_ignore_ascii_case(origin))
    })
}

pub fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Cross-Origin-Opener-Policy: same-origin\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; \
         connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

fn find(headers: &[httparse::Header<'_>], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| value(h.value))
}

fn value(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(extra: &str) -> Vec<u8> {
        format!("GET /ws HTTP/1.1\r\nHost: 127.0.0.1:7878\r\n{extra}\r\n").into_bytes()
    }

    #[test]
    fn parses_a_websocket_request() {
        let parsed = parse_head(&head(
            "Upgrade: WebSocket\r\nOrigin: http://127.0.0.1:7878\r\n",
        ))
        .unwrap()
        .unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.path, "/ws");
        assert_eq!(parsed.host.as_deref(), Some("127.0.0.1:7878"));
        assert_eq!(parsed.origin.as_deref(), Some("http://127.0.0.1:7878"));
        assert!(parsed.wants_websocket);
    }

    #[test]
    fn a_plain_request_does_not_want_a_websocket() {
        let parsed = parse_head(&head("")).unwrap().unwrap();
        assert!(!parsed.wants_websocket);
        assert_eq!(parsed.origin, None);
    }

    #[test]
    fn an_incomplete_head_is_not_an_error() {
        assert_eq!(parse_head(b"GET / HTTP/1.1\r\nHost: x"), Ok(None));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(
            parse_head(b"\x16\x03\x01\x02\x00\r\n\r\n"),
            Err(HeadError::Malformed)
        );
    }

    #[test]
    fn rejects_two_host_headers() {
        assert_eq!(
            parse_head(&head("Host: evil.example\r\n")),
            Err(HeadError::DuplicateHost)
        );
    }

    #[test]
    fn rejects_too_many_headers() {
        let many: String = (0..40)
            .map(|i| ["X-", &i.to_string(), ": a\r\n"].concat())
            .collect();
        assert_eq!(parse_head(&head(&many)), Err(HeadError::Malformed));
    }

    #[test]
    fn only_local_hosts_are_allowed() {
        assert!(host_allowed(Some("127.0.0.1:7878"), 7878));
        assert!(host_allowed(Some("LOCALHOST:7878"), 7878));
        assert!(host_allowed(Some("[::1]:7878"), 7878));
        assert!(!host_allowed(Some("127.0.0.1:9"), 7878));
        assert!(!host_allowed(Some("evil.example:7878"), 7878));
        assert!(!host_allowed(Some("127.0.0.1"), 7878));
        assert!(!host_allowed(None, 7878));
    }

    #[test]
    fn only_local_origins_are_allowed() {
        assert!(origin_allowed(None, 7878));
        assert!(origin_allowed(Some("http://127.0.0.1:7878"), 7878));
        assert!(origin_allowed(Some("http://localhost:7878"), 7878));
        assert!(!origin_allowed(Some("https://127.0.0.1:7878"), 7878));
        assert!(!origin_allowed(Some("http://evil.example"), 7878));
        assert!(!origin_allowed(
            Some("http://127.0.0.1:7878.evil.example"),
            7878
        ));
        assert!(!origin_allowed(Some("null"), 7878));
    }

    #[test]
    fn responses_carry_the_security_headers() {
        let text = String::from_utf8(response("200 OK", "text/html", b"hi")).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Content-Security-Policy: default-src 'none'"));
        assert!(text.contains("X-Content-Type-Options: nosniff"));
        assert!(text.ends_with("\r\n\r\nhi"));
    }

    #[test]
    fn the_head_parser_never_panics_and_respects_its_limits() {
        let mut rng = crate::testing::Rng::new(0x5EED_0001_2345_6789);
        let valid = head("Upgrade: websocket\r\nOrigin: http://127.0.0.1:7878\r\n");
        let mut parsed = 0;
        for round in 0..30_000 {
            let input = if round % 3 == 0 {
                rng.bytes(400)
            } else {
                rng.mutate(&valid)
            };
            if let Ok(Some(request)) = parse_head(&input) {
                parsed += 1;
                assert_ne!(request.method, "");
                assert!(request.path.len() <= MAX_HEAD_LEN);
            }
        }
        assert!(parsed > 100);
    }
}
