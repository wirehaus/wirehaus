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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exposure {
    hosts: Vec<String>,
    origins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposureError(pub String);

impl std::fmt::Display for ExposureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExposureError {}

impl Exposure {
    pub fn new(public_hosts: &[String], viewer_origins: &[String]) -> Result<Self, ExposureError> {
        let mut hosts = Vec::new();
        let mut origins = Vec::new();
        for host in public_hosts {
            let host = valid_authority(host)?;
            origins.push(format!("https://{host}"));
            hosts.push(host);
        }
        for origin in viewer_origins {
            let authority = origin.strip_prefix("https://").ok_or_else(|| {
                ExposureError(format!(
                    "{origin:?} must start with https:// and name only a host"
                ))
            })?;
            origins.push(format!("https://{}", valid_authority(authority)?));
        }
        Ok(Self { hosts, origins })
    }

    #[must_use]
    pub fn host_allowed(&self, host: Option<&str>, port: u16) -> bool {
        host.is_some_and(|host| {
            allowed_authorities(port)
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
                || self
                    .hosts
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(host))
        })
    }

    #[must_use]
    pub fn origin_allowed(&self, origin: Option<&str>, port: u16) -> bool {
        origin.is_none_or(|origin| {
            allowed_authorities(port)
                .iter()
                .any(|allowed| format!("http://{allowed}").eq_ignore_ascii_case(origin))
                || self
                    .origins
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(origin))
        })
    }
}

fn valid_authority(text: &str) -> Result<String, ExposureError> {
    let invalid =
        |reason: &str| ExposureError(format!("{text:?} is not a valid public host: {reason}"));
    let (name, port) = match text.rsplit_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (text, None),
    };
    if let Some(port) = port
        && (port.is_empty()
            || port.parse::<u16>().ok().is_none_or(|p| p == 0)
            || port.starts_with('0'))
    {
        return Err(invalid("the port must be between 1 and 65535"));
    }
    let labels: Vec<&str> = name.split('.').collect();
    let well_formed = name.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if !well_formed {
        return Err(invalid("use a plain DNS name such as agent.example.com"));
    }
    if labels
        .iter()
        .all(|label| label.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("an IP address is not accepted"));
    }
    let name = name.to_ascii_lowercase();
    Ok(match port {
        Some(port) => format!("{name}:{port}"),
        None => name,
    })
}

fn allowed_authorities(port: u16) -> [String; 3] {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
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
         Cross-Origin-Resource-Policy: same-origin\r\n\
         Permissions-Policy: camera=(self), microphone=(), geolocation=(), payment=(), usb=()\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; \
         manifest-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

pub fn json_response(status: &str, body: &str, allow_origin: Option<&str>) -> Vec<u8> {
    let cors = match allow_origin {
        Some(origin) => format!(
            "Access-Control-Allow-Origin: {origin}\r\n\
             Cross-Origin-Resource-Policy: cross-origin\r\n"
        ),
        None => "Cross-Origin-Resource-Policy: same-origin\r\n".to_owned(),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Vary: Origin\r\n\
         {cors}\
         Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body.as_bytes());
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
    fn only_local_hosts_are_allowed_by_default() {
        let local = Exposure::default();
        assert!(local.host_allowed(Some("127.0.0.1:7878"), 7878));
        assert!(local.host_allowed(Some("LOCALHOST:7878"), 7878));
        assert!(local.host_allowed(Some("[::1]:7878"), 7878));
        assert!(!local.host_allowed(Some("127.0.0.1:9"), 7878));
        assert!(!local.host_allowed(Some("evil.example:7878"), 7878));
        assert!(!local.host_allowed(Some("127.0.0.1"), 7878));
        assert!(!local.host_allowed(None, 7878));
    }

    #[test]
    fn only_local_origins_are_allowed_by_default() {
        let local = Exposure::default();
        assert!(local.origin_allowed(None, 7878));
        assert!(local.origin_allowed(Some("http://127.0.0.1:7878"), 7878));
        assert!(local.origin_allowed(Some("http://localhost:7878"), 7878));
        assert!(!local.origin_allowed(Some("https://127.0.0.1:7878"), 7878));
        assert!(!local.origin_allowed(Some("http://evil.example"), 7878));
        assert!(!local.origin_allowed(Some("http://127.0.0.1:7878.evil.example"), 7878));
        assert!(!local.origin_allowed(Some("null"), 7878));
    }

    fn exposure(hosts: &[&str], origins: &[&str]) -> Result<Exposure, ExposureError> {
        let own = |items: &[&str]| {
            items
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>()
        };
        Exposure::new(&own(hosts), &own(origins))
    }

    #[test]
    fn a_public_host_is_accepted_exactly_and_brings_its_own_https_origin() {
        let open = exposure(&["Agent.Example.com"], &[]).unwrap();
        assert!(open.host_allowed(Some("agent.example.com"), 7878));
        assert!(open.host_allowed(Some("AGENT.example.COM"), 7878));
        assert!(open.host_allowed(Some("127.0.0.1:7878"), 7878));
        for other in [
            "example.com",
            "evil.agent.example.com",
            "agent.example.com:443",
            "agent.example.com.evil.net",
        ] {
            assert!(!open.host_allowed(Some(other), 7878), "{other}");
        }
        assert!(open.origin_allowed(Some("https://agent.example.com"), 7878));
        assert!(!open.origin_allowed(Some("http://agent.example.com"), 7878));
        assert!(!open.origin_allowed(Some("https://app.example.com"), 7878));
        assert!(!open.origin_allowed(Some("https://agent.example.com.evil.net"), 7878));
    }

    #[test]
    fn a_separate_viewer_origin_may_open_the_connection_without_being_a_host() {
        let open = exposure(&["agent.example.com"], &["https://app.example.com"]).unwrap();
        assert!(open.origin_allowed(Some("https://app.example.com"), 7878));
        assert!(!open.host_allowed(Some("app.example.com"), 7878));
        assert!(!open.origin_allowed(Some("https://app.example.com:8443"), 7878));
        let custom = exposure(&[], &["https://app.example.com:8443"]).unwrap();
        assert!(custom.origin_allowed(Some("https://app.example.com:8443"), 7878));
    }

    #[test]
    fn unsafe_or_sloppy_exposure_settings_are_rejected() {
        for host in [
            "",
            "*",
            "*.example.com",
            "localhost",
            "example",
            "http://agent.example.com",
            "agent.example.com/",
            "agent.example.com/path",
            "agent..example.com",
            ".example.com",
            "example.com.",
            "-a.example.com",
            "a-.example.com",
            "a b.example.com",
            "127.0.0.1",
            "10.0.0.5",
            "[::1]",
            "agent.example.com:",
            "agent.example.com:0",
            "agent.example.com:99999",
            "agent.example.com:08",
            "agent_x.example.com",
            "ünï.example.com",
        ] {
            assert!(exposure(&[host], &[]).is_err(), "{host:?} was accepted");
        }
        for origin in [
            "http://app.example.com",
            "app.example.com",
            "https://",
            "https://*.example.com",
            "https://app.example.com/",
            "https://app.example.com/x",
            "https://user@app.example.com",
            "https://127.0.0.1",
        ] {
            assert!(exposure(&[], &[origin]).is_err(), "{origin:?} was accepted");
        }
    }
}
