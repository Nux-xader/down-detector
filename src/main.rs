use axum::{extract::Query, routing::get, Json, Router};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

const MAX_UNIQUE_DOMAINS: usize = 50;
const CONCURRENCY: usize = 50;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct Params {
    #[serde(default)]
    domains: String,
}

#[derive(Serialize)]
struct DomainResult {
    domain: String,
    is_down: bool,
    status_code: Option<u16>,
}

#[derive(Serialize)]
struct ApiResponse {
    msg: &'static str,
    data: Vec<DomainResult>,
}

struct Item {
    raw: String,
    url: String,
    key: String,
}

fn normalize(raw: &str) -> Option<Item> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let url = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    match parsed.host_str() {
        Some(host) if !host.is_empty() => {}
        _ => return None,
    }
    Some(Item {
        key: url.to_lowercase(),
        raw: trimmed.to_string(),
        url,
    })
}

fn collect_domains(param: &str) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in param.split(',') {
        let Some(item) = normalize(raw) else { continue };
        if !seen.insert(item.key.clone()) {
            continue;
        }
        out.push(item);
        if out.len() >= MAX_UNIQUE_DOMAINS {
            break;
        }
    }
    out
}

fn is_up(status: u16) -> bool {
    status < 500
}

fn down_from(status: Option<u16>) -> bool {
    status.is_none_or(|s| !is_up(s))
}

async fn check_item(client: &reqwest::Client, item: Item) -> DomainResult {
    let status_code = match client.get(&item.url).send().await {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(_) => None,
    };
    DomainResult {
        domain: item.raw,
        is_down: down_from(status_code),
        status_code,
    }
}

async fn check(Query(params): Query<Params>) -> Json<ApiResponse> {
    let items = collect_domains(&params.domains);

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("down-detector/0.1")
        .build()
        .expect("failed to build http client");

    let mut results: Vec<(usize, DomainResult)> = stream::iter(items.into_iter().enumerate())
        .map(|(i, item)| {
            let client = client.clone();
            async move { (i, check_item(&client, item).await) }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    results.sort_by_key(|(i, _)| *i);

    Json(ApiResponse {
        msg: "success",
        data: results.into_iter().map(|(_, r)| r).collect(),
    })
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(check));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind 0.0.0.0:3000");
    axum::serve(listener, app).await.expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains(param: &str) -> Vec<String> {
        collect_domains(param).into_iter().map(|i| i.raw).collect()
    }

    #[test]
    fn filters_invalid() {
        assert_eq!(
            domains("example.com, ,not a domain,ftp://foo.com,https://"),
            vec!["example.com"]
        );
    }

    #[test]
    fn accepts_scheme_port_path() {
        assert_eq!(
            domains("https://Example.com:8443/Path,192.168.1.1:8080"),
            vec!["https://Example.com:8443/Path", "192.168.1.1:8080"]
        );
    }

    #[test]
    fn dedup_case_insensitive() {
        assert_eq!(
            domains("Google.com,google.com,https://GOOGLE.com"),
            vec!["Google.com"]
        );
    }

    #[test]
    fn caps_at_50_unique() {
        let param = (0..60).map(|i| format!("d{i}.com")).collect::<Vec<_>>().join(",");
        assert_eq!(domains(&param).len(), 50);
    }

    #[test]
    fn status_rules() {
        assert!(is_up(200) && is_up(301) && is_up(404));
        assert!(!is_up(500) && !is_up(503));
        assert!(!down_from(Some(200)) && !down_from(Some(301)) && !down_from(Some(404)));
        assert!(down_from(Some(500)) && down_from(None));
    }
}
