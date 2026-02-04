use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

pub(crate) const RATE_LIMIT_MAX_RETRIES: usize = 5;
pub(crate) const RATE_LIMIT_BASE_DELAY: Duration = Duration::from_secs(2);
pub(crate) const RATE_LIMIT_MAX_DELAY: Duration = Duration::from_secs(60);

pub(crate) fn is_rate_limited(status: StatusCode, body: &str) -> bool {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    let code = status.as_u16();
    if code == 529 || code == 503 {
        return true;
    }
    let lower = body.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("resource_exhausted")
        || lower.contains("quota")
        || lower.contains("overloaded")
}

pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    None
}

pub(crate) async fn wait_with_backoff(
    provider: &str,
    attempt: usize,
    delay: Duration,
    retry_after: Option<Duration>,
) -> Duration {
    let mut wait = delay;
    if let Some(retry_after) = retry_after
        && retry_after > wait
    {
        wait = retry_after;
    }
    warn!(
        "{} rate limited; retrying in {:.1}s (attempt {}/{})",
        provider,
        wait.as_secs_f32(),
        attempt,
        RATE_LIMIT_MAX_RETRIES
    );
    sleep(wait).await;
    next_delay(delay)
}

pub(crate) fn next_delay(current: Duration) -> Duration {
    let next_secs = current
        .as_secs()
        .saturating_mul(2)
        .max(RATE_LIMIT_BASE_DELAY.as_secs());
    let next = Duration::from_secs(next_secs);
    if next > RATE_LIMIT_MAX_DELAY {
        RATE_LIMIT_MAX_DELAY
    } else {
        next
    }
}
