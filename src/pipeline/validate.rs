use crate::core::answer::Source;
use crate::pipeline::{LinkStatus, SearchEvent};
use futures::stream::{self, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

const CONCURRENT_CHECKS: usize = 8;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_REDIRECTS: usize = 5;

pub fn classify_status(code: u16) -> LinkStatus {
    match code {
        200..=399 => LinkStatus::Valid,
        _ => LinkStatus::Invalid(code),
    }
}

pub fn should_retry_with_get(code: u16) -> bool {
    matches!(code, 403 | 405 | 501)
}

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

fn build_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent(concat!("faro/", env!("CARGO_PKG_VERSION")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_maps_http_codes_to_link_health() {
        struct Case {
            name: &'static str,
            code: u16,
            want: LinkStatus,
        }
        let cases = [
            Case {
                name: "ok",
                code: 200,
                want: LinkStatus::Valid,
            },
            Case {
                name: "redirect already followed",
                code: 301,
                want: LinkStatus::Valid,
            },
            Case {
                name: "not found",
                code: 404,
                want: LinkStatus::Invalid(404),
            },
            Case {
                name: "gone",
                code: 410,
                want: LinkStatus::Invalid(410),
            },
            Case {
                name: "server error",
                code: 500,
                want: LinkStatus::Invalid(500),
            },
        ];
        for case in cases {
            assert_eq!(classify_status(case.code), case.want, "{}", case.name);
        }
    }

    #[test]
    fn head_unfriendly_codes_trigger_a_ranged_get_retry() {
        struct Case {
            name: &'static str,
            code: u16,
            want: bool,
        }
        let cases = [
            Case {
                name: "forbidden often blocks head only",
                code: 403,
                want: true,
            },
            Case {
                name: "method not allowed",
                code: 405,
                want: true,
            },
            Case {
                name: "not implemented",
                code: 501,
                want: true,
            },
            Case {
                name: "ok needs no retry",
                code: 200,
                want: false,
            },
            Case {
                name: "not found needs no retry",
                code: 404,
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(should_retry_with_get(case.code), case.want, "{}", case.name);
        }
    }
}
