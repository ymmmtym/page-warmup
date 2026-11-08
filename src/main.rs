use headless_chrome::{
    Browser, LaunchOptions,
    protocol::cdp::{Network, Page, types::Event},
};
use serde::Serialize;
use std::env;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const WARMUP_TIMEOUT: Duration = Duration::from_secs(180);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Serialize)]
struct WarmupResult {
    time: String,
    status: Option<u16>,
    readiness: Option<String>,
}

fn navigate_until_dom_content_loaded(
    tab: &headless_chrome::Tab,
    url: &str,
    dom_content_loaded: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    while dom_content_loaded.try_recv().is_ok() {}
    tab.navigate_to(url)?;
    dom_content_loaded
        .recv_timeout(timeout)
        .map_err(|_| "DOMContentLoaded を待機中にタイムアウトしました")?;
    Ok(())
}

fn is_headless_mode(value: Option<&str>) -> bool {
    value == Some("true")
}

fn header_value<'a>(headers: Option<&'a serde_json::Value>, name: &str) -> Option<&'a str> {
    headers?
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_str())
}

fn normalize_not_ready_value(value: Option<String>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "asleep".to_string())
}

fn is_ready(status: u16, readiness: Option<&str>, not_ready_value: &str) -> bool {
    status < 500 && readiness != Some(not_ready_value)
}

fn print_last_result(last_result: &Option<WarmupResult>) -> Result<(), serde_json::Error> {
    if let Some(result) = last_result {
        println!("{}", serde_json::to_string(result)?);
    }
    Ok(())
}

fn finish_with_timeout(
    last_result: &Option<WarmupResult>,
) -> Result<(), Box<dyn std::error::Error>> {
    print_last_result(last_result)?;
    Err(format!(
        "{}秒以内にページの応答を確認できませんでした",
        WARMUP_TIMEOUT.as_secs()
    )
    .into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default()).init();

    let url = env::var("TARGET_URL").map_err(|_| "TARGET_URL を指定してください")?;

    let headless = is_headless_mode(env::var("HEADLESS").ok().as_deref());

    let readiness_header = env::var("READINESS_HEADER")
        .ok()
        .filter(|value| !value.is_empty());
    if readiness_header.is_none() {
        eprintln!("READINESS_HEADER is not set; readiness detection disabled (using status only)");
    }
    let not_ready_value = normalize_not_ready_value(env::var("NOT_READY_VALUE").ok());
    if let Some(header) = readiness_header.as_deref() {
        eprintln!("Readiness header: {header} (not ready value: {not_ready_value})");
    }

    let deadline = Instant::now() + WARMUP_TIMEOUT;
    eprintln!("Starting Chromium (headless={headless})");
    let browser = Browser::new(LaunchOptions {
        headless,
        // Let Chromium select an ephemeral debugging port.
        port: Some(0),
        ..Default::default()
    })?;
    eprintln!("Chromium started");

    let tab = browser.new_tab()?;
    tab.set_default_timeout(WARMUP_TIMEOUT);
    eprintln!("Browser tab created");

    let navigation_response = Arc::new(Mutex::new(None));
    let response_for_listener = Arc::clone(&navigation_response);
    let response_url = url.clone();
    let (dom_content_loaded_tx, dom_content_loaded_rx) = mpsc::channel();

    tab.call_method(Network::Enable {
        max_total_buffer_size: None,
        max_resource_buffer_size: None,
        max_post_data_size: None,
        report_direct_socket_traffic: None,
        enable_durable_messages: None,
    })?;
    tab.call_method(Page::Enable {
        enable_file_chooser_opened_event: None,
    })?;
    eprintln!("CDP domains enabled");
    tab.add_event_listener(Arc::new(move |event: &Event| match event {
        Event::NetworkResponseReceived(event) => {
            let response = &event.params.response;
            if event.params.Type == Network::ResourceType::Document && response.url == response_url
            {
                let readiness = readiness_header
                    .as_deref()
                    .and_then(|name| header_value(response.headers.0.as_ref(), name))
                    .map(str::to_string);
                *response_for_listener.lock().unwrap() = Some((response.status as u16, readiness));
            }
        }

        Event::PageDomContentEventFired(_) => {
            let _ = dom_content_loaded_tx.send(());
        }
        _ => {}
    }))?;

    // First navigation (trigger warm-up)
    eprintln!("Opening target URL in Chromium");
    navigate_until_dom_content_loaded(
        &tab,
        &url,
        &dom_content_loaded_rx,
        deadline.saturating_duration_since(Instant::now()),
    )?;
    eprintln!("Initial page reached DOMContentLoaded");
    let mut check_delay = FIRST_CHECK_DELAY;
    let mut last_result = None;
    let mut attempt = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return finish_with_timeout(&last_result);
        }

        thread::sleep(check_delay.min(remaining));
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return finish_with_timeout(&last_result);
        }

        // Check by navigating in the browser; a plain HTTP request does not warm the page up.
        tab.set_default_timeout(remaining);
        *navigation_response.lock().unwrap() = None;
        attempt += 1;
        eprintln!("Warmup check attempt {attempt}");
        if let Err(error) =
            navigate_until_dom_content_loaded(&tab, &url, &dom_content_loaded_rx, remaining)
        {
            eprintln!("Warmup check navigation failed: {error}");
            print_last_result(&last_result)?;
            return Err(error);
        }

        let navigation_result = navigation_response.lock().unwrap().take();
        let (status, readiness) = match navigation_result {
            Some(result) => result,
            None => {
                print_last_result(&last_result)?;
                return Err("ブラウザーのナビゲーション応答を取得できませんでした".into());
            }
        };
        let result = WarmupResult {
            time: chrono::Utc::now().to_rfc3339(),
            status: Some(status),
            readiness: readiness.clone(),
        };
        eprintln!("Warmup check response: status={status}, readiness={readiness:?}");

        if is_ready(status, readiness.as_deref(), &not_ready_value) {
            println!("{}", serde_json::to_string(&result)?);
            return Ok(());
        }

        last_result = Some(result.clone());
        if Instant::now() >= deadline {
            return finish_with_timeout(&last_result);
        }
        check_delay = RETRY_INTERVAL;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WarmupResult, header_value, is_headless_mode, is_ready, normalize_not_ready_value,
    };

    #[test]
    fn defaults_to_headed_mode() {
        assert!(!is_headless_mode(None));
        assert!(!is_headless_mode(Some("false")));
    }

    #[test]
    fn true_enables_headless_mode() {
        assert!(is_headless_mode(Some("true")));
    }

    #[test]
    fn status_below_500_without_readiness_is_ready() {
        assert!(is_ready(200, None, "asleep"));
        assert!(is_ready(404, None, "asleep"));
    }

    #[test]
    fn status_below_500_with_non_asleep_readiness_is_ready() {
        assert!(is_ready(200, Some("waking"), "asleep"));
        assert!(is_ready(404, Some("failed"), "asleep"));
    }

    #[test]
    fn asleep_readiness_is_not_ready() {
        assert!(!is_ready(503, Some("asleep"), "asleep"));
        assert!(!is_ready(200, Some("asleep"), "asleep"));
    }

    #[test]
    fn status_500_or_above_is_not_ready() {
        assert!(!is_ready(500, None, "asleep"));
        assert!(!is_ready(503, Some("failed"), "asleep"));
        assert!(!is_ready(503, Some("waking"), "asleep"));
    }

    #[test]
    fn custom_not_ready_value_is_respected() {
        assert!(!is_ready(200, Some("sleeping"), "sleeping"));
        assert!(is_ready(200, Some("asleep"), "sleeping"));
    }

    #[test]
    fn not_ready_value_defaults_to_asleep() {
        assert_eq!(normalize_not_ready_value(None), "asleep");
        assert_eq!(normalize_not_ready_value(Some(String::new())), "asleep");
        assert_eq!(
            normalize_not_ready_value(Some("sleeping".to_string())),
            "sleeping"
        );
    }

    #[test]
    fn header_value_matches_case_insensitively() {
        let headers = serde_json::json!({ "X-Example-Readiness": "asleep" });
        assert_eq!(
            header_value(Some(&headers), "x-example-readiness"),
            Some("asleep")
        );
    }

    #[test]
    fn header_value_is_none_when_missing() {
        let headers = serde_json::json!({});
        assert_eq!(header_value(Some(&headers), "x-example-readiness"), None);
        assert_eq!(header_value(None, "x-example-readiness"), None);
    }

    #[test]
    fn warmup_result_keeps_stdout_json_contract() {
        let result = WarmupResult {
            time: "2026-01-01T00:00:00+00:00".to_string(),
            status: Some(200),
            readiness: None,
        };

        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            r#"{"time":"2026-01-01T00:00:00+00:00","status":200,"readiness":null}"#
        );
    }
}
