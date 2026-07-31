use crate::core::answer::Answer;
use crate::core::citations::image_urls;
use crate::pipeline::SearchEvent;
use crate::pipeline::validate::build_client;
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc::Sender;

const CONCURRENT_FETCHES: usize = 4;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

pub async fn fetch_images(answer: &Answer, tx: &Sender<SearchEvent>) {
    let urls = image_urls(answer);
    if urls.is_empty() {
        return;
    }
    let Some(client) = build_client() else {
        return;
    };
    stream::iter(urls)
        .map(|url| fetch_one(client.clone(), url))
        .buffer_unordered(CONCURRENT_FETCHES)
        .for_each(|(url, bytes)| async move {
            let _ = tx.send(SearchEvent::ImageFetched { url, bytes }).await;
        })
        .await;
}

async fn fetch_one(client: reqwest::Client, url: String) -> (String, Option<Vec<u8>>) {
    let bytes = download(&client, &url).await;
    (url, bytes)
}

async fn download(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let too_large = response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES as u64);
    if too_large {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() <= MAX_IMAGE_BYTES).then(|| bytes.to_vec())
}
