// INPUT:  crate::SandboxStoreData, alva_sandbox_abi DTOs/limits, reqwest::blocking, serde_json, std::{io, net, str, time}, wasmtime
// OUTPUT: register_http_proxy (crate), validate_allowed_domain_pattern
// POS:    Host-enforced blocking HTTP fetch bridge with fail-closed domain grants and per-hop redirect validation.

use crate::SandboxStoreData;
use alva_sandbox_abi::{
    FetchHeader, FetchProxyResult, FetchRequest, FetchResponse, FETCH_PROXY_ABI_VERSION,
    MAX_FETCH_PROXY_REQUEST_BYTES, MAX_FETCH_PROXY_RESPONSE_BYTES, MAX_FETCH_REQUEST_BODY_BYTES,
    MAX_FETCH_RESPONSE_BODY_BYTES,
};
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode, Url};
use std::io::{self, Read};
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;
use wasmtime::{Caller, Extern, Linker};

const MAX_REDIRECTS: usize = 10;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
enum DomainRule {
    Exact(String),
    Subdomains(String),
}

#[derive(Debug, Clone)]
struct DomainAllowlist(Vec<DomainRule>);

impl DomainAllowlist {
    fn new(patterns: Vec<String>) -> Result<Self, String> {
        patterns
            .into_iter()
            .map(|pattern| parse_domain_rule(&pattern))
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    fn permits_url(&self, url: &Url) -> Result<(), String> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!(
                "fetch URL scheme {:?} is not allowed; use http or https",
                url.scheme()
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("fetch URL credentials are not allowed".to_string());
        }
        let host = url
            .host_str()
            .ok_or_else(|| "fetch URL has no host".to_string())?;
        let host = normalize_host(host);
        let permitted = self.0.iter().any(|rule| match rule {
            DomainRule::Exact(allowed) => host == *allowed,
            DomainRule::Subdomains(apex) => {
                host.len() > apex.len() + 1
                    && host.ends_with(apex)
                    && host.as_bytes()[host.len() - apex.len() - 1] == b'.'
            }
        });
        if permitted {
            Ok(())
        } else {
            Err(format!(
                "fetch host {host:?} is not in the job domain allowlist"
            ))
        }
    }
}

/// Validate one CLI/job allowlist pattern using the host's canonical matching
/// rules. Entries are ASCII hostnames or IP literals without schemes, paths,
/// credentials, or ports; `*.example.com` grants subdomains but not the apex.
pub fn validate_allowed_domain_pattern(pattern: &str) -> Result<(), String> {
    parse_domain_rule(pattern).map(|_| ())
}

fn parse_domain_rule(pattern: &str) -> Result<DomainRule, String> {
    let normalized = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() || normalized != pattern.trim().trim_end_matches('.') {
        if pattern.trim().is_empty() {
            return Err("domain allowlist entry cannot be empty".to_string());
        }
        // ASCII case is normalized deliberately; non-ASCII lowercasing can
        // change byte shape and must be supplied as explicit IDNA/punycode.
        if !pattern.is_ascii() {
            return Err(format!(
                "domain allowlist entry {pattern:?} must be ASCII (use IDNA/punycode)"
            ));
        }
    }
    if !pattern.is_ascii() {
        return Err(format!(
            "domain allowlist entry {pattern:?} must be ASCII (use IDNA/punycode)"
        ));
    }
    let (wildcard, host) = normalized
        .strip_prefix("*.")
        .map_or((false, normalized.as_str()), |host| (true, host));
    if IpAddr::from_str(host).is_ok() {
        return if wildcard {
            Err(format!(
                "IP allowlist entry {pattern:?} cannot use a wildcard"
            ))
        } else {
            Ok(DomainRule::Exact(host.to_string()))
        };
    }
    if host.is_empty()
        || host.contains(['/', ':', '?', '#', '@'])
        || host.chars().any(char::is_whitespace)
        || host.contains('*')
    {
        return Err(format!(
            "invalid domain allowlist entry {pattern:?}; expected a host or *.host without scheme, path, or port"
        ));
    }
    if host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(format!("invalid domain allowlist hostname {pattern:?}"));
    }
    Ok(if wildcard {
        DomainRule::Subdomains(host.to_string())
    } else {
        DomainRule::Exact(host.to_string())
    })
}

fn normalize_host(host: &str) -> String {
    host.trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

pub(crate) fn register_http_proxy(
    linker: &mut Linker<SandboxStoreData>,
    allowed_domains: Vec<String>,
) -> Result<(), wasmtime::Error> {
    let allowlist = DomainAllowlist::new(allowed_domains).map_err(wasmtime::Error::msg)?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .build()
        .map_err(|error| wasmtime::Error::msg(format!("build fetch HTTP client: {error}")))?;

    linker.func_wrap(
        "alva:host/http",
        "fetch",
        move |mut caller: Caller<'_, SandboxStoreData>, req_ptr: i32, req_len: i32| {
            let request = read_request(&mut caller, req_ptr, req_len)?;
            let result = match execute_fetch(&client, &allowlist, request) {
                Ok(response) => FetchProxyResult::success(response),
                Err(error) => FetchProxyResult::failure(error),
            };
            write_result(&mut caller, &result)
        },
    )?;
    Ok(())
}

fn read_request(
    caller: &mut Caller<'_, SandboxStoreData>,
    req_ptr: i32,
    req_len: i32,
) -> Result<FetchRequest, wasmtime::Error> {
    let req_start = usize::try_from(req_ptr)
        .map_err(|_| wasmtime::Error::msg("negative fetch request pointer"))?;
    let req_len = usize::try_from(req_len)
        .map_err(|_| wasmtime::Error::msg("negative fetch request length"))?;
    if req_len > MAX_FETCH_PROXY_REQUEST_BYTES {
        return Err(wasmtime::Error::msg(format!(
            "fetch request is {req_len} bytes; limit is {MAX_FETCH_PROXY_REQUEST_BYTES} bytes"
        )));
    }
    let req_end = req_start
        .checked_add(req_len)
        .ok_or_else(|| wasmtime::Error::msg("fetch request range overflow"))?;
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
    let encoded = memory
        .data(&*caller)
        .get(req_start..req_end)
        .ok_or_else(|| wasmtime::Error::msg("fetch request range is outside guest memory"))?;
    let request: FetchRequest = serde_json::from_slice(encoded)
        .map_err(|error| wasmtime::Error::msg(format!("decode fetch request: {error}")))?;
    if !request.has_supported_version() {
        return Err(wasmtime::Error::msg(format!(
            "unsupported fetch request version {}; host supports {}",
            request.version, FETCH_PROXY_ABI_VERSION
        )));
    }
    if request.body.len() > MAX_FETCH_REQUEST_BODY_BYTES {
        return Err(wasmtime::Error::msg(format!(
            "fetch request body is {} bytes; limit is {MAX_FETCH_REQUEST_BODY_BYTES} bytes",
            request.body.len()
        )));
    }
    Ok(request)
}

fn write_result(
    caller: &mut Caller<'_, SandboxStoreData>,
    result: &FetchProxyResult,
) -> Result<i64, wasmtime::Error> {
    let mut encoded = BoundedJsonBuffer::new(MAX_FETCH_PROXY_RESPONSE_BYTES);
    if let Err(error) = serde_json::to_writer(&mut encoded, result) {
        return Err(wasmtime::Error::msg(if encoded.exceeded {
            format!("fetch response exceeds the {MAX_FETCH_PROXY_RESPONSE_BYTES}-byte JSON limit")
        } else {
            format!("encode fetch response: {error}")
        }));
    }
    let response = encoded.bytes;
    let resp_len = i32::try_from(response.len())
        .map_err(|_| wasmtime::Error::msg("fetch response exceeds ptr/len ABI limit"))?;
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
    let alloc = caller
        .get_export("alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export alloc"))?
        .typed::<i32, i32>(&caller)?;
    let resp_ptr = alloc.call(&mut *caller, resp_len)?;
    let resp_start = usize::try_from(resp_ptr)
        .map_err(|_| wasmtime::Error::msg("guest alloc returned a negative pointer"))?;
    memory.write(&mut *caller, resp_start, &response)?;
    let packed = (u64::from(resp_ptr as u32) << 32) | u64::from(resp_len as u32);
    Ok(packed as i64)
}

fn execute_fetch(
    client: &Client,
    allowlist: &DomainAllowlist,
    request: FetchRequest,
) -> Result<FetchResponse, String> {
    let mut url =
        Url::parse(&request.url).map_err(|error| format!("invalid fetch URL: {error}"))?;
    let mut method = Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("invalid fetch method {:?}: {error}", request.method))?;
    let mut headers = request_headers(request.headers)?;
    let mut body = request.body;

    for redirect_count in 0..=MAX_REDIRECTS {
        // This check is deliberately inside the loop and occurs before every
        // send. The reqwest client has redirects disabled, so a 3xx cannot
        // move the socket to an unvalidated host behind our back.
        allowlist.permits_url(&url)?;
        let response = client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .body(body.clone())
            .send()
            .map_err(|error| format!("fetch {url}: {error}"))?;

        if !is_redirect(response.status()) {
            return read_response(response);
        }
        let Some(location) = response.headers().get(LOCATION) else {
            return read_response(response);
        };
        if redirect_count == MAX_REDIRECTS {
            return Err(format!("fetch exceeded {MAX_REDIRECTS} redirects"));
        }
        let location = location
            .to_str()
            .map_err(|error| format!("redirect Location is not valid text: {error}"))?;
        let next = url
            .join(location)
            .map_err(|error| format!("invalid redirect target {location:?}: {error}"))?;
        // Validate now as well as at the top of the next iteration so the
        // rejection is guaranteed before any request construction or send.
        allowlist.permits_url(&next)?;

        if should_switch_to_get(response.status(), &method) {
            method = Method::GET;
            body.clear();
            headers.remove(reqwest::header::CONTENT_LENGTH);
            headers.remove(reqwest::header::CONTENT_TYPE);
        }
        if !same_origin(&url, &next) {
            headers.remove(reqwest::header::AUTHORIZATION);
            headers.remove(reqwest::header::COOKIE);
            headers.remove(reqwest::header::PROXY_AUTHORIZATION);
        }
        url = next;
    }
    unreachable!("redirect loop returns at its configured bound")
}

fn request_headers(headers: Vec<FetchHeader>) -> Result<HeaderMap, String> {
    let mut result = HeaderMap::new();
    for header in headers {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|error| format!("invalid request header name {:?}: {error}", header.name))?;
        let value = HeaderValue::from_str(&header.value).map_err(|error| {
            format!(
                "invalid value for request header {:?}: {error}",
                header.name
            )
        })?;
        result.append(name, value);
    }
    Ok(result)
}

fn read_response(mut response: Response) -> Result<FetchResponse, String> {
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| FetchHeader {
            name: name.as_str().to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    let mut body = Vec::new();
    response
        .by_ref()
        .take((MAX_FETCH_RESPONSE_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("read fetch response body: {error}"))?;
    if body.len() > MAX_FETCH_RESPONSE_BODY_BYTES {
        return Err(format!(
            "fetch response body exceeds the {MAX_FETCH_RESPONSE_BODY_BYTES}-byte limit"
        ));
    }
    Ok(FetchResponse::new(status, headers, body))
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn should_switch_to_get(status: StatusCode, method: &Method) -> bool {
    (status == StatusCode::SEE_OTHER && *method != Method::HEAD)
        || ((status == StatusCode::MOVED_PERMANENTLY || status == StatusCode::FOUND)
            && *method == Method::POST)
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str().map(normalize_host) == right.host_str().map(normalize_host)
        && left.port_or_known_default() == right.port_or_known_default()
}

struct BoundedJsonBuffer {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedJsonBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl io::Write for BoundedJsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "fetch proxy JSON exceeds byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowlist(patterns: &[&str]) -> DomainAllowlist {
        DomainAllowlist::new(patterns.iter().map(|value| value.to_string()).collect()).unwrap()
    }

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn empty_allowlist_denies_every_host() {
        let error = allowlist(&[])
            .permits_url(&url("https://example.com"))
            .unwrap_err();
        assert!(error.contains("not in the job domain allowlist"), "{error}");
    }

    #[test]
    fn exact_rules_ignore_case_and_port_but_not_subdomains() {
        let rules = allowlist(&["Example.COM"]);
        assert!(rules
            .permits_url(&url("https://example.com:8443/a"))
            .is_ok());
        assert!(rules.permits_url(&url("https://EXAMPLE.com/a")).is_ok());
        assert!(rules
            .permits_url(&url("https://www.example.com/a"))
            .is_err());
    }

    #[test]
    fn wildcard_rules_match_subdomains_but_not_the_apex() {
        let rules = allowlist(&["*.example.com"]);
        assert!(rules.permits_url(&url("https://a.example.com")).is_ok());
        assert!(rules
            .permits_url(&url("https://deep.a.example.com"))
            .is_ok());
        assert!(rules.permits_url(&url("https://example.com")).is_err());
        assert!(rules.permits_url(&url("https://notexample.com")).is_err());
    }

    #[test]
    fn exact_ip_literals_are_supported_for_local_jobs() {
        let rules = allowlist(&["127.0.0.1", "::1"]);
        assert!(rules.permits_url(&url("http://127.0.0.1:8080")).is_ok());
        assert!(rules.permits_url(&url("http://[::1]:8080")).is_ok());
        assert!(validate_allowed_domain_pattern("*.127.0.0.1").is_err());
    }

    #[test]
    fn patterns_reject_scheme_path_port_credentials_and_non_ascii() {
        for invalid in [
            "https://example.com",
            "example.com/path",
            "example.com:443",
            "user@example.com",
            "éxample.com",
            "",
        ] {
            assert!(
                validate_allowed_domain_pattern(invalid).is_err(),
                "{invalid:?} should be invalid"
            );
        }
    }
}
