use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub delay_ms: u64,
}

impl RetryConfig {
    pub fn startup() -> Self {
        Self {
            max_attempts: 30,
            delay_ms: 1000,
        }
    }
}

pub fn require_non_empty_connection(
    url: String,
    label: &str,
    env_var: &str,
) -> Result<String> {
    if url.trim().is_empty() {
        anyhow::bail!("{} is not configured (set {} environment variable)", label, env_var);
    }
    Ok(url)
}

pub fn wait_for_postgres(label: &str, url: &str, retry: RetryConfig) -> Result<()> {
    for attempt in 1..=retry.max_attempts {
        match std::net::TcpStream::connect(extract_host_port(url, 5432)) {
            Ok(_) => {
                println!("{}: postgres reachable (attempt {})", label, attempt);
                return Ok(());
            }
            Err(_) if attempt < retry.max_attempts => {
                println!("{}: waiting for postgres (attempt {}/{})", label, attempt, retry.max_attempts);
                std::thread::sleep(std::time::Duration::from_millis(retry.delay_ms));
            }
            Err(e) => {
                anyhow::bail!("{}: postgres unreachable after {} attempts: {}", label, retry.max_attempts, e);
            }
        }
    }
    Ok(())
}

pub fn wait_for_http_health(
    label: &str,
    base_url: &str,
    path: &str,
    _expected_body: Option<&str>,
    retry: RetryConfig,
) -> Result<()> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    for attempt in 1..=retry.max_attempts {
        match ureq::get(&url).call() {
            Ok(_) => {
                println!("{}: healthy at {} (attempt {})", label, url, attempt);
                return Ok(());
            }
            Err(_) if attempt < retry.max_attempts => {
                println!("{}: waiting for {} (attempt {}/{})", label, url, attempt, retry.max_attempts);
                std::thread::sleep(std::time::Duration::from_millis(retry.delay_ms));
            }
            Err(e) => {
                anyhow::bail!("{}: unhealthy after {} attempts: {}", label, retry.max_attempts, e);
            }
        }
    }
    Ok(())
}

fn extract_host_port(url: &str, default_port: u16) -> String {
    let without_scheme = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .unwrap_or(url);
    let after_at = without_scheme
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    let host_port = after_at.split('/').next().unwrap_or(after_at);
    if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{}:{}", host_port, default_port)
    }
}
