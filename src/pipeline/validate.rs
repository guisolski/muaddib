use crate::core::answer::Source;
use crate::core::citations::{LinkStatus, classify_status, should_retry_with_get};
use crate::pipeline::SearchEvent;
use futures::stream::{self, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

const CONCURRENT_CHECKS: usize = 8;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_REDIRECTS: usize = 5;
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub async fn validate_links(sources: &[Source], tx: &Sender<SearchEvent>) {
    let Some(client) = build_client() else {
        return;
    };
    let check_futures: Vec<_> = sources
        .iter()
        .map(|source| check_source(&client, source))
        .collect();
    stream::iter(check_futures)
        .buffer_unordered(CONCURRENT_CHECKS)
        .for_each(|(source_id, status)| async move {
            let _ = tx
                .send(SearchEvent::LinkChecked { source_id, status })
                .await;
        })
        .await;
}

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

async fn check_source(client: &reqwest::Client, source: &Source) -> (u32, LinkStatus) {
    (source.id, link_status(client, &source.url).await)
}

async fn link_status(client: &reqwest::Client, url: &str) -> LinkStatus {
    match client.head(url).send().await {
        Ok(response) => {
            let code = response.status().as_u16();
            if should_retry_with_get(code) {
                ranged_get_status(client, url).await
            } else {
                classify_status(code)
            }
        }
        Err(_) => LinkStatus::Unreachable,
    }
}

async fn ranged_get_status(client: &reqwest::Client, url: &str) -> LinkStatus {
    match client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
    {
        Ok(response) => classify_status(response.status().as_u16()),
        Err(_) => LinkStatus::Unreachable,
    }
}
