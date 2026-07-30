use crate::core::answer::{Answer, Block};
use crate::pipeline::SearchEvent;
use crate::pipeline::validate::build_client;
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc::Sender;

const CONCURRENT_FETCHES: usize = 4;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

pub fn image_urls(answer: &Answer) -> Vec<String> {
    let mut urls = Vec::new();
    for block in &answer.blocks {
        if let Block::Image { url, .. } = block
            && !urls.contains(url)
        {
            urls.push(url.clone());
        }
    }
    urls
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::Emphasis;

    fn image_block(url: &str) -> Block {
        Block::Image {
            url: url.to_string(),
            caption: String::new(),
            source_ids: vec![],
            emphasis: Emphasis::None,
        }
    }

    #[test]
    fn image_urls_are_unique_and_in_document_order() {
        struct Case {
            name: &'static str,
            blocks: Vec<Block>,
            want: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "no image blocks yield nothing",
                blocks: vec![Block::Unknown],
                want: vec![],
            },
            Case {
                name: "duplicates collapse keeping first position",
                blocks: vec![
                    image_block("https://img.example/a.png"),
                    Block::Unknown,
                    image_block("https://img.example/b.png"),
                    image_block("https://img.example/a.png"),
                ],
                want: vec!["https://img.example/a.png", "https://img.example/b.png"],
            },
        ];
        for case in cases {
            let answer = Answer {
                blocks: case.blocks,
                ..Answer::default()
            };
            assert_eq!(image_urls(&answer), case.want, "{}", case.name);
        }
    }
}
