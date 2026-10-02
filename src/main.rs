use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};
use tokio::time::{self, Instant, MissedTickBehavior};

const MAX_PROMPT_BODY_CHARS: usize = 12_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ApiResult {
    status_code: Option<u16>,
    body: String,
    error: Option<String>,
}

impl ApiResult {
    fn is_error(&self) -> bool {
        self.error.is_some()
            || self
                .status_code
                .is_some_and(|code| !(200..300).contains(&code))
    }
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<Msg<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct Msg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResp {
    message: RespMsg,
}

#[derive(Deserialize)]
struct RespMsg {
    content: String,
}

struct Config {
    api_url: String,
    poll_interval: Duration,
    analysis_interval: Duration,
    ollama_url: String,
    ollama_model: String,
    request_timeout: Duration,
}

impl Config {
    fn from_env() -> Result<Self> {
        Ok(Self {
            api_url: env::var("API_URL")
                .unwrap_or_else(|_| "http://localhost:8000/api/status".to_owned()),
            poll_interval: Duration::from_secs(env_u64("POLL_INTERVAL_SECS", 300)?),
            analysis_interval: Duration::from_secs(env_u64("ANALYSIS_INTERVAL_SECS", 3600)?),
            ollama_url: env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434/api/chat".to_owned()),
            ollama_model: env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_owned()),
            request_timeout: Duration::from_secs(env_u64("REQUEST_TIMEOUT_SECS", 10)?),
        })
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("환경 변수 {name}은 0보다 큰 정수여야 합니다"))
            .and_then(|parsed| {
                if parsed == 0 {
                    anyhow::bail!("환경 변수 {name}은 0보다 커야 합니다")
                } else {
                    Ok(parsed)
                }
            }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("환경 변수 {name}을 읽지 못했습니다")),
    }
}

async fn fetch_api_data(client: &Client, url: &str) -> ApiResult {
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) => {
            return ApiResult {
                status_code: None,
                body: String::new(),
                error: Some(error.to_string()),
            };
        }
    };

    let status = response.status();
    match response.text().await {
        Ok(body) => ApiResult {
            status_code: Some(status.as_u16()),
            body,
            error: None,
        },
        Err(error) => ApiResult {
            status_code: Some(status.as_u16()),
            body: String::new(),
            error: Some(format!("응답 본문을 읽지 못했습니다: {error}")),
        },
    }
}

async fn analyze(
    client: &Client,
    ollama_url: &str,
    model: &str,
    current: &ApiResult,
    previous: Option<&ApiResult>,
) -> Result<String> {
    let current_description = describe_result(current);
    let previous_description = previous
        .map(describe_result)
        .unwrap_or_else(|| "이전 데이터가 없습니다 (최초 실행).".to_owned());
    let prompt = format!(
        "당신은 API 모니터링 분석 에이전트입니다. 아래 데이터를 쉬운 한국어로 설명하세요.\
         \n- 먼저 위험도를 정상/주의/경고 중 하나로 표시하세요.\
         \n- 핵심 상태와 주요 수치를 설명하고, 이전 데이터와 달라진 점을 비교하세요.\
         \n- 이상징후나 HTTP 오류가 있으면 원인으로 추정되는 점과 확인할 사항을 알려주세요.\
         \n- 우선 3줄 이내로 간결하게 답변하세요.\
         \n\n[현재 API 결과]\n{current_description}\n\n[이전 API 결과]\n{previous_description}"
    );

    let request = ChatReq {
        model,
        messages: vec![Msg {
            role: "user",
            content: &prompt,
        }],
        stream: false,
    };

    let response = client
        .post(ollama_url)
        .json(&request)
        .send()
        .await
        .with_context(|| format!("Ollama 요청 실패 ({ollama_url})"))?;
    let status = response.status();
    if status != StatusCode::OK {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Ollama가 HTTP {status}를 반환했습니다: {body}");
    }

    let response = response
        .json::<ChatResp>()
        .await
        .context("Ollama 응답을 해석하지 못했습니다")?;
    Ok(response.message.content)
}

fn describe_result(result: &ApiResult) -> String {
    let body = truncate_chars(&result.body, MAX_PROMPT_BODY_CHARS);
    let body = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or(body);
    let truncated = result.body.chars().count() > MAX_PROMPT_BODY_CHARS;

    serde_json::json!({
        "status_code": result.status_code,
        "error": result.error,
        "body": body,
        "body_truncated": truncated,
    })
    .to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

async fn run_agent(config: Config) -> Result<()> {
    let api_client = Client::builder()
        .timeout(config.request_timeout)
        .build()
        .context("API HTTP 클라이언트를 만들지 못했습니다")?;
    let ollama_client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("Ollama HTTP 클라이언트를 만들지 못했습니다")?;
    let mut interval = time::interval(config.poll_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut previous: Option<ApiResult> = None;
    let mut last_analysis: Option<Instant> = None;

    println!("API 모니터링 에이전트를 시작합니다.");
    println!("대상 API: {}", config.api_url);
    println!("폴링 주기: {}초", config.poll_interval.as_secs());
    println!("Ollama 모델: {}", config.ollama_model);

    loop {
        interval.tick().await;
        let now = Instant::now();
        println!("\n[Agent] API 데이터를 수집합니다...");
        let current = fetch_api_data(&api_client, &config.api_url).await;

        let changed = previous.as_ref() != Some(&current);
        let is_error = current.is_error();
        let periodic_analysis_due =
            last_analysis.is_none_or(|last| now.duration_since(last) >= config.analysis_interval);

        if changed || is_error || periodic_analysis_due {
            println!("[Agent] Ollama로 응답을 분석합니다...");
            match analyze(
                &ollama_client,
                &config.ollama_url,
                &config.ollama_model,
                &current,
                previous.as_ref(),
            )
            .await
            {
                Ok(report) => {
                    println!(
                        "\n=== API 모니터링 리포트 ===\n{report}\n==========================="
                    );
                    last_analysis = Some(Instant::now());
                }
                Err(error) => eprintln!("[Agent] AI 분석 실패: {error:#}"),
            }
        } else {
            println!("[Agent] 변경 사항이 없어 AI 분석을 건너뜁니다.");
        }

        previous = Some(current);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    run_agent(config).await
}
