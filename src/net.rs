//! Bounded synchronous HTTP. Redirect credentials never cross origins.
use crate::domain::*;
use reqwest::{blocking::Client, header};
use std::io::Read;
use std::time::{Duration, Instant, SystemTime};

pub struct Http {
    client: Client,
    loopback: Client,
    pub offline: bool,
}
pub struct Response {
    pub bytes: Vec<u8>,
    pub content_type: String,
}
impl Http {
    pub fn new(offline: bool) -> Result<Self> {
        let build = |no_proxy| {
            let mut builder = Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("skill-bom/", env!("CARGO_PKG_VERSION")));
            if no_proxy {
                builder = builder.no_proxy();
            }
            builder
                .build()
                .map_err(|_| Error::new("NETWORK", "Cannot initialize TLS client", 2))
        };
        Ok(Self {
            client: build(false)?,
            loopback: build(true)?,
            offline,
        })
    }
    pub fn get(&self, url: &str, token: Option<&str>) -> Result<Response> {
        if self.offline {
            return Err(Error::new(
                "OFFLINE_MISS",
                "Network access prohibited in offline mode",
                2,
            ));
        }
        let original =
            url::Url::parse(url).map_err(|_| Error::new("URL", "Invalid request URL", 2))?;
        let mut current = original.clone();
        let started = Instant::now();
        let mut retries = 0;
        let mut redirects = 0;
        loop {
            crate::process::check_interrupt()?;
            if started.elapsed() >= Duration::from_secs(90) {
                return Err(Error::new(
                    "NETWORK_TIMEOUT",
                    "HTTP time budget exceeded",
                    2,
                ));
            }
            let client = if matches!(
                current.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
            ) {
                &self.loopback
            } else {
                &self.client
            };
            let mut request = client.get(current.clone());
            if current.origin() == original.origin()
                && let Some(t) = token
            {
                request = request.bearer_auth(t);
            }
            let mut res = match request.send() {
                Ok(response) => response,
                Err(error) if (error.is_connect() || error.is_timeout()) && retries < 2 => {
                    retries += 1;
                    std::thread::sleep(Duration::from_millis(100 * retries));
                    continue;
                }
                Err(_) => return Err(Error::new("NETWORK","HTTP request failed or timed out",2).phase("download").hint("Check connectivity and retry; credentials are read from the configured environment variable.")),
            };
            if res.status().is_redirection() {
                redirects += 1;
                if redirects > 5 {
                    return Err(Error::new("PROTOCOL", "Too many redirects", 2));
                }
                let loc = res
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| Error::new("PROTOCOL", "Redirect has no location", 2))?;
                let next = current
                    .join(loc)
                    .map_err(|_| Error::new("PROTOCOL", "Invalid redirect", 2))?;
                if next.scheme() != "https"
                    && !(next.scheme() == "http"
                        && matches!(next.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
                    || !next.username().is_empty()
                    || next.password().is_some()
                {
                    return fail("PROTOCOL", "Unsafe redirect destination");
                }
                current = next;
                continue;
            }
            if res.status().as_u16() == 429 || res.status().is_server_error() {
                if retries >= 2 {
                    return Err(Error::new(
                        "HTTP_RETRY_EXHAUSTED",
                        format!("HTTP {} after bounded retries", res.status().as_u16()),
                        2,
                    ));
                }
                let wait = res
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(retry_after)
                    .unwrap_or(Duration::from_secs(1 << retries));
                if started.elapsed() + wait > Duration::from_secs(90) {
                    return Err(Error::new(
                        "RATE_LIMIT",
                        "Retry-After exceeds request budget",
                        2,
                    ));
                }
                let until = Instant::now() + wait;
                while Instant::now() < until {
                    crate::process::check_interrupt()?;
                    std::thread::sleep(
                        Duration::from_millis(50)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                }
                retries += 1;
                continue;
            }
            if !res.status().is_success() {
                let status = res.status().as_u16();
                // Error bodies can echo credentials; expose only bounded status/protocol data.
                return Err(Error::new(if matches!(status,403|410|423) {"SOURCE_BLOCKED"} else if status==404 {"SOURCE_VERSION_UNAVAILABLE"} else {"HTTP_STATUS"},format!("HTTP {status} (server rejected request; JSON and text errors are supported)"),if status>=500 {2} else {1}).phase("download"));
            }
            if res.content_length().is_some_and(|n| n > MAX_DOWNLOAD) {
                return Err(Error::new(
                    "RESOURCE_LIMIT",
                    "HTTP response exceeds download limit",
                    2,
                ));
            }
            let content_type = res
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let mut bytes = vec![];
            (&mut res).take(MAX_DOWNLOAD + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_DOWNLOAD {
                return Err(Error::new(
                    "RESOURCE_LIMIT",
                    "HTTP response exceeds download limit",
                    2,
                ));
            }
            return Ok(Response {
                bytes,
                content_type,
            });
        }
    }
    pub fn json(&self, url: &str, token: Option<&str>) -> Result<serde_json::Value> {
        let response = self.get(url, token)?;
        serde_json::from_slice(&response.bytes)
            .map_err(|_| Error::new("PROTOCOL", "Expected JSON response", 2))
    }
}
fn retry_after(s: &str) -> Option<Duration> {
    s.parse().ok().map(Duration::from_secs).or_else(|| {
        httpdate::parse_http_date(s)
            .ok()
            .map(|t| t.duration_since(SystemTime::now()).unwrap_or_default())
    })
}

/// Injectable HTTP boundary used by source contract tests and embedding applications.
pub trait Transport {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Response>;
    fn json(&self, url: &str, token: Option<&str>) -> Result<serde_json::Value> {
        let response = self.get(url, token)?;
        serde_json::from_slice(&response.bytes)
            .map_err(|_| Error::new("PROTOCOL", "Expected JSON response", 2))
    }
}
impl Transport for Http {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Response> {
        Http::get(self, url, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_after_supports_dates_and_invalid_values() {
        assert_eq!(retry_after("0"), Some(Duration::ZERO));
        assert_eq!(
            retry_after("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_after("invalid"), None);
        assert_eq!(
            retry_after(&httpdate::fmt_http_date(
                SystemTime::now() + Duration::from_secs(60)
            ))
            .unwrap()
            .as_secs(),
            59
        );
    }
}
