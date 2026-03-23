use tiny_http::{Header, Response, Server, StatusCode};

pub(crate) struct HttpServer {
    server: Server,
}

impl HttpServer {
    pub fn new(port: u16) -> Result<Self, std::io::Error> {
        let addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&addr)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
        Ok(Self { server })
    }

    pub fn into_inner(self) -> Server {
        self.server
    }
}

pub(crate) fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let len = data.len();
    let cursor = std::io::Cursor::new(data);
    Response::new(
        StatusCode(status),
        vec![Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()],
        cursor,
        Some(len),
        None,
    )
}

pub(crate) fn error_response(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::json!({"error": message}).to_string();
    json_response(status, &body)
}

pub(crate) fn read_body(request: &mut tiny_http::Request) -> Result<String, std::io::Error> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

pub(crate) fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
