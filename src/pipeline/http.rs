use std::time::Duration;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const MAX_REDIRECTS: usize = 5;
pub(crate) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub(crate) fn build_client() -> Option<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
        ),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent(BROWSER_USER_AGENT)
        .default_headers(headers)
        .build()
        .ok()
}
