use crate::core::answer::Source;
use crate::core::citations::{LinkStatus, classify_status, should_retry_with_get};
use crate::pipeline::SearchEvent;
use crate::pipeline::http::build_client;
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc::Sender;

const CONCURRENT_CHECKS: usize = 8;

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
