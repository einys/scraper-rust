use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use regex::Regex;
use schemars::schema_for;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::fs;

use crate::lib;
use crate::lib::dyn_result::DynError;
use crate::lib::dyn_result::DynResult;
use crate::lib::page::ExtractedContent;
use crate::lib::page::ExtractedContentWrapper;
use crate::lib::page::SelectorSchema;
use crate::lib::page::TargetItem;
use crate::PageTrait;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::Ollama;

use serde_json::Value;

const MAX_HISTORY_COUNT: usize = 12;

/// ActionHistory 출력 규칙을 캡슐화한 헬퍼 구조체
struct HistoryFormatter<'a> {
    step: &'a str,
    action_name: &'a str,
    current_url: &'a str,
}

impl<'a> HistoryFormatter<'a> {
    fn new(step: &'a str, action_name: &'a str, current_location_url: &'a str) -> Self {
        Self {
            step,
            action_name,
            current_url: current_location_url,
        }
    }

    /// ActionHistory 표준 응답 문자열 포맷 생성
    fn format(&self, status: &str, reasoning: &str, data_payload: &str) -> String {
        format!(
            "{{ step: \"{}\", action_name: \"{}\", status: \"{}\", current_url: \"{}\" }} <data>{}</data>",
            self.step, self.action_name, status, self.current_url, data_payload
        )
    }
}

/// 문자열 형태의 숫자("1000")도 정수(1000)로 자동 변환해 주는 custom deserializer
fn deserialize_number_from_string<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Value::deserialize(deserializer)?;
    match val {
        Value::Number(n) => n
            .as_i64()
            .map(|v| v as i32)
            .ok_or_else(|| serde::de::Error::custom("Invalid number")),
        Value::String(s) => s.parse::<i32>().map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom(
            "Expected number or numeric string",
        )),
    }
}

fn default_scroll_dy() -> i32 {
    1000
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", content = "params")]
pub enum AgentAction {
    /// 파일을 읽어옵니다.
    #[serde(alias = "read", alias = "read_file", alias = "Read")]
    ReadFile { reasoning: String, filename: String },
    /// 파일에 텍스트 내용을 쓰거나 추가합니다.
    #[serde(alias = "write", alias = "write_file", alias = "Write")]
    WriteFile {
        reasoning: String, // 👈 생각 과정 추가
        filename: String,
        content: String,
    },
    /// 수직으로 페이지를 스크롤합니다.
    #[serde(alias = "scroll", alias = "scroll_page", alias = "Scroll")]
    ScrollPage {
        reasoning: String, // 👈 생각 과정 추가
        #[serde(
            default = "default_scroll_dy",
            deserialize_with = "deserialize_number_from_string"
        )]
        dy: i32,
    },
    /// 특정 CSS 셀렉터 요소를 클릭합니다.
    #[serde(alias = "click", alias = "click_element", alias = "Click")]
    ClickElement {
        reasoning: String,
        selector: String,
        target_url: String,
        x: Option<f64>,
        y: Option<f64>,
    },
    /// 모든 수집 목표를 완료하고 에이전트를 종료합니다.
    #[serde(alias = "finish", alias = "finish_agent", alias = "Finish")]
    Finish {
        reasoning: String, // 👈 생각 과정 추가
        reason: String,
    },
    /// DOM에서 원하는 셀렉터 목록으로 데이터를 추출합니다.
    // #[serde(alias = "extract", alias = "extract_data", alias = "Extract")]
    // ExtractData { reasoning: String },
    /// go back to the previous page.
    #[serde(alias = "back", alias = "back_page", alias = "Back")]
    BackPage {
        // New variant for going back
        reasoning: String, // 👈 thinking process
    },
}
impl AgentAction {
    // reasoning 필드만 쉽게 뽑아내는 헬퍼 메서드
    pub fn reasoning(&self) -> &str {
        match self {
            AgentAction::ReadFile { reasoning, .. } => reasoning,
            AgentAction::WriteFile { reasoning, .. } => reasoning,
            AgentAction::ScrollPage { reasoning, .. } => reasoning,
            AgentAction::ClickElement { reasoning, .. } => reasoning,
            AgentAction::Finish { reasoning, .. } => reasoning,
            // AgentAction::ExtractData { reasoning, .. } => reasoning,
            AgentAction::BackPage { reasoning } => reasoning,
        }
    }

    /// 액션 이름(enum variant 이름)을 반환하는 헬퍼 메서드
    pub fn name(&self) -> &'static str {
        match self {
            AgentAction::ReadFile { .. } => "ReadFile",
            AgentAction::WriteFile { .. } => "WriteFile",
            AgentAction::ScrollPage { .. } => "ScrollPage",
            AgentAction::ClickElement { .. } => "ClickElement",
            AgentAction::Finish { .. } => "Finish",
            // AgentAction::ExtractData { .. } => "ExtractData",
            AgentAction::BackPage { .. } => "BackPage",
        }
    }

    /// 히스토리에 기록할 주요 파라미터 요약 정보
    pub fn to_string(&self) -> String {
        match self {
            AgentAction::ReadFile { filename, .. } => format!("ReadFile({})", filename),
            AgentAction::WriteFile { filename, .. } => format!("WriteFile({})", filename),
            AgentAction::ScrollPage { dy, .. } => format!("ScrollPage(dy={})", dy),
            AgentAction::ClickElement {
                selector,
                target_url,
                x,
                y,
                ..
            } => format!(
                "ClickElement(selector={}, url={} x={:?}, y={:?})",
                selector, target_url, x, y
            ),
            AgentAction::Finish { reason, .. } => format!("Finish({})", reason),
            // AgentAction::ExtractData { .. } => {
            //     format!("ExtractData")
            // }
            AgentAction::BackPage { .. } => "BackPage".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentObservation {
    pub simplified_dom: String,
    pub html_content: String,
    pub current_url: String,
    pub focused_selectors: Option<SelectorSchema>,
    pub target_elements: Vec<TargetItem>,
    // 필요 시 추가 가능 (예: pub current_url: String, pub screenshot: Vec<u8>)
}

pub enum LoopControl {
    Continue,
    Finish,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionHistory {
    pub step: usize,
    pub reasoning: String,
    pub action_name: String,
    pub status: String,       // 예: "Success", "Failed: Element not found"
    pub data: Option<String>, // Optional data associated with the action
}
impl ActionHistory {
    pub fn to_prompt_string(&self) -> String {
        format!(
            "Step {}: Action='{}' | Status='{}'\n  Reasoning: {}\n  Data: {}",
            self.step,
            self.action_name,
            self.status,
            self.reasoning,
            self.data.as_ref().map_or("None".to_string(), |d| d.clone())
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TargetItemWrapper {
    // 1. 최상위 배열 형태: [ TargetItem, ... ] (가장 직관적)
    Array(Vec<TargetItem>),
    // 2. 단일 객체 형태: { "selector": "...", ... }
    Single(TargetItem),
}

impl TargetItemWrapper {
    pub fn into_vec(self) -> Vec<TargetItem> {
        match self {
            TargetItemWrapper::Array(arr) => arr,
            TargetItemWrapper::Single(item) => vec![item],
        }
    }
}

/// URL 중복 체크 시 비교 정확도를 높이기 위한 정규화 헬퍼
fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/')
        .split('#') // 해시 태그 제거
        .next()
        .unwrap_or(url)
        .to_string()
}

pub struct WebAgent<'a, P: PageTrait> {
    ollama: Ollama,
    model: String,
    page: &'a P,
    system_task_prompt: String,
    page_observation_system_prompt: String,
    // 💡 스텝별 행동 기록을 누적 보관하는 필드 추가
    history: Vec<ActionHistory>,
    max_history_depth: usize, // 토큰 폭발 방지를 위한 최근 기록 유지 개수
    pub visited_urls: HashSet<String>,
    target_webpage: String,
}

impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// Agent 생성자
    pub fn new(
        ollama_host: &str,
        ollama_port: u16,
        model: &str,
        page: &'a P,
        system_task_prompt: &str,
        page_element_target_description: &str,
        target_webpage: &str,
    ) -> Self {
        let ollama = Ollama::builder()
            .host(ollama_host.to_string())
            .port(ollama_port)
            .build();
        Self {
            ollama,
            model: model.to_string(),
            page,
            system_task_prompt: system_task_prompt.to_string(),
            page_observation_system_prompt: page_element_target_description.to_string(),
            history: Vec::new(),
            max_history_depth: MAX_HISTORY_COUNT, // 기본적으로 최근 8개의 이전 스텝을 저장합니다.
            visited_urls: HashSet::new(),
            target_webpage: target_webpage.to_string(),
        }
    }

    /// URL을 방문 처리할 때 쿼리 파라미터/트레일링 슬래시 정규화 후 저장
    pub fn mark_visited(&mut self, url: &str) {
        let normalized = normalize_url(url);
        self.visited_urls.insert(normalized);
    }

    pub fn is_visited(&self, url: &str) -> bool {
        let normalized = normalize_url(url);
        let is_visited = self.visited_urls.contains(&normalized);
        println!(
            "Debug: URL {:?} is visited: {} in {:?}",
            url, is_visited, self.visited_urls
        );
        is_visited
    }

    // 💡 max_history_depth를 고려하여 오래된 history부터 제거하는 메소드 (FIFO 형식)
    pub fn trim_history(&mut self) {
        if self.history.len() > self.max_history_depth {
            let overflow = self.history.len() - self.max_history_depth;
            self.history.drain(0..overflow);
        }
    }

    pub async fn append_to_file(&self, filename: &str, data: serde_json::Value) -> DynResult<()> {
        let path = Path::new(filename);

        // 1. 기존 파일이 존재하면 읽어서 Vec<serde_json::Value>로 파싱, 없으면 새 Vec 생성
        let mut items: Vec<serde_json::Value> = if path.exists() {
            let content = fs::read_to_string(path).await.unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_else(|_| Vec::new())
        } else {
            Vec::new()
        };

        // 2. 새 아이템 추가
        items.push(data);

        // 3. 예쁘게 포맷팅된 JSON 문자열로 직렬화 후 비동기 쓰기
        let updated_json = serde_json::to_string_pretty(&items)?;
        fs::write(path, updated_json).await?;

        Ok(())
    }

    /// URL을 안전한 파일 경로 형태로 변환하여 Simplified DOM을 저장
    pub async fn save_simplified_dom(&self, current_url: &str, dom_content: &str) -> DynResult<()> {
        // 1. "https://" 및 "http://" 프로토콜 prefix 제거
        let cleaned_url = current_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        // 2. 파일명 및 디렉터리 경로 구성
        // 예: "cmsn.info" -> "cmsn.info_simple.html"
        // 예: "cmsn.info/i/xRsyP" -> "cmsn.info/i/xRsyP_simple.html"
        let relative_path_str = if cleaned_url.is_empty() {
            "data/root_simple.html".to_string()
        } else {
            format!("data/{}_simple.html", cleaned_url)
        };

        let file_path = Path::new(&relative_path_str);

        // 3. 디렉터리가 포함되어 있다면 해당 상위 디렉터리를 일괄 자동 생성 (create_dir_all)
        if let Some(parent_dir) = file_path.parent() {
            if !parent_dir.as_os_str().is_empty() && !parent_dir.exists() {
                fs::create_dir_all(parent_dir).await?;
                println!("📁 디렉터리 자동 생성 완료: {:?}", parent_dir);
            }
        }

        // 4. 파일 비동기 작성 (덮어쓰기)
        fs::write(&file_path, dom_content).await?;
        println!("💾 Simplified DOM 저장 완료: {:?}", file_path);

        Ok(())
    }

    /// URL을 기준으로 추출된 데이터를 비동기로 JSON 저장
    pub async fn save_scraped_data(
        &self,
        current_url: &str,
        data: &ExtractedContentWrapper,
    ) -> DynResult<String> {
        let cleaned_url = current_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        let relative_path_str = if cleaned_url.is_empty() {
            "data/root_data.json".to_string()
        } else {
            format!("data/{}_data.json", cleaned_url)
        };

        let file_path = Path::new(&relative_path_str);

        // 상위 디렉터리 자동 생성
        if let Some(parent_dir) = file_path.parent() {
            if !parent_dir.as_os_str().is_empty() && !parent_dir.exists() {
                tokio::fs::create_dir_all(parent_dir).await?;
                println!("📁 데이터 저장 디렉터리 자동 생성: {:?}", parent_dir);
            }
        }

        // 비동기 JSON 직렬화 및 저장 (tokio::fs 활용으로 동기 블로킹 방지)
        let json_bytes = serde_json::to_vec_pretty(data)?;
        tokio::fs::write(&file_path, json_bytes).await?;
        println!("💾 추출 데이터 저장 완료: {:?}", file_path);

        Ok(relative_path_str)
    }

    /// 현재 페이지 상태를 관측하고, 최신 DOM 기반으로 Selector 및 Target Items 캡처
    pub async fn capture_observation(&mut self) -> DynResult<AgentObservation> {
        let current_url = self.page.current_url().await.unwrap_or_default();
        println!("👀 Observing current page: {}", current_url);

        // 💡 SPA/비동기 DOM 렌더링 완성 대기
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // 1. 새 탭 열림 무력화 JS 실행
        let sanitize_js = r#"
                (() => {
                    document.querySelectorAll('a[target]').forEach(el => el.removeAttribute('target'));
                    window.open = function(url) {
                        if (url) { window.location.href = url; }
                        return window;
                    };
                })();
            "#;
        let _ = self.page.evaluate(sanitize_js).await;

        // 2. 초기 Simplified DOM 캡처
        let simplified_dom = crate::lib::page::get_simplified_dom_tree(self.page).await?;
        let html_content = self.page.content().await?;

        // 3. 셀렉터 추출 및 타겟 항목 수집 (최대 3회 재시도)

        let mut attempt = 0;
        let max_selector_retries = 3;
        let (selectors, target_elements) = loop {
            if let Ok(s) = self.ask_important_selectors(&simplified_dom).await {
                if let Ok(items) = lib::page::get_target_items(self.page, &s).await {
                    if !items.is_empty() {
                        break (s, items); // 👈 루프 탈출과 동시에 튜플 반환!
                    }
                }
            }
            attempt += 1;

            if attempt >= max_selector_retries {
                // 💥 3회 시도 실패 시

                println!("⚠️ [Fallback Selector] 0개 요소 수집됨. 범용 카드 링크 셀렉터(a[href*='/i/'])로 fallback 시도");
                let mut fallback_schema = SelectorSchema::default();
                fallback_schema.fields.insert(
                    "item_link".to_string(),
                    "a[href*='/i/'], main a".to_string(),
                );

                if let Ok(fallback_items) =
                    lib::page::get_target_items(self.page, &fallback_schema).await
                {
                    break (fallback_schema, fallback_items);
                } else {
                    eprintln!("⚠️ [Fallback Selector] a[href] fallback 시도에도 여전히 어떤 element도 관찰되지 않음. 빈 배열 반환.");
                    // default
                    break (SelectorSchema::default(), Vec::new());
                }
            }

            eprintln!("⚠️ [Selector Retry] 수집된 DOM 요소가 0개입니다. 셀렉터를 다시 추출합니다. ({}/{})",attempt, max_selector_retries);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        };

        Ok(AgentObservation {
            simplified_dom,
            html_content,
            current_url,
            focused_selectors: Some(selectors),
            target_elements,
        })
    }

    // 히스토리에서 <Data></Data> 패턴을 모두 삭제하고 action 기록만 남깁니다.
    /// 히스토리의 status 메시지에서 <data>...</data> 패턴을 모두 제거하고 action 기록만 남깁니다.
    pub fn clean_history_data(&mut self) {
        // (?s) 플래그: 개행 문자를 포함하여 <data>...</data> 내부 전체를 매칭
        let re = Regex::new(r"(?s)\s*<data>.*?</data>").unwrap();

        for item in &mut self.history {
            item.status = re.replace_all(&item.status, "").trim().to_string();
        }

        println!("🧹 [Memory Clean] 히스토리 내 <data> 태그 영역 제거 완료 (액션 기록만 유지)");
    }

    /// schemars 스키마 기반 Action 가이드 생성 (Nested params 구조 대응)
    fn build_compact_action_guide(&self) -> String {
        let full_schema = schema_for!(AgentAction);
        let schema_val = serde_json::to_value(&full_schema).unwrap_or_default();

        let mut compact_docs = String::new();

        if let Some(one_of) = schema_val.get("oneOf").and_then(|v| v.as_array()) {
            for item in one_of {
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if let Some(props) = item.get("properties").and_then(|p| p.as_object()) {
                    let mut action_name = None;

                    // 1. enum/const 속성을 찾아 액션 이름을 자동 추출
                    for (_key, val) in props {
                        if let Some(enum_val) = val
                            .get("enum")
                            .and_then(|e| e.get(0))
                            .and_then(|v| v.as_str())
                        {
                            action_name = Some(enum_val.to_string());
                            break;
                        }
                        if let Some(const_val) = val.get("const").and_then(|v| v.as_str()) {
                            action_name = Some(const_val.to_string());
                            break;
                        }
                    }

                    let action_name_str = action_name.as_deref().unwrap_or("Unknown");

                    // 2. nested된 "params" 객체 내부의 실제 파라미터 필드 수집
                    let mut param_fields = Vec::new();
                    if let Some(params_obj) = props
                        .get("params")
                        .or_else(|| props.get("Params"))
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.as_object())
                    {
                        for (field, _) in params_obj {
                            param_fields.push(field.as_str());
                        }
                    } else {
                        // 만약 params 속성이 없는 단일 필드 variant일 경우 (Fallback)
                        for (k, _) in props {
                            if k != "action" && k != "action_name" && k != "Params" && k != "params"
                            {
                                param_fields.push(k.as_str());
                            }
                        }
                    }

                    compact_docs.push_str(&format!(
                        "- action: `{}`\n  description: {}\n  params: {}\n\n",
                        action_name_str,
                        description,
                        serde_json::to_string(&param_fields).unwrap_or_default()
                    ));
                }
            }
        }

        compact_docs
    }

    fn history_to_string(&self) -> String {
        self.history
            .iter()
            .map(|h| h.to_prompt_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 프롬프트 전달용: <data>...</data> 영역이 제거된 히스토리 요약 문자열 반환
    // pub fn history_summary(&self, limit: usize) -> String {
    //     let re = Regex::new(r"(?s)\s*<data>.*?</data>").unwrap();

    //     self.history
    //         .iter()
    //         .rev()
    //         .take(limit)
    //         .rev()
    //         .map(|h| {
    //             let cleaned_status = re.replace_all(&h.status, "").trim().to_string();
    //             format!(
    //                 "Step {}: Action='{}' | Status='{}'",
    //                 h.step, h.action_name, cleaned_status
    //             )
    //         })
    //         .collect::<Vec<_>>()
    //         .join("\n")
    // }

    /// LLM에 다음 단계를 추론하도록 요청하는 헬퍼 메서드
    async fn ask_step(&self, obs: &AgentObservation) -> DynResult<AgentAction> {
        let action_guide = self.build_compact_action_guide();

        println!("LLM에 다음 단계를 추론하도록 요청합니다.");
        println!(
            "목록에 포함된 Actions List:\n{}...",
            action_guide.chars().take(200).collect::<String>()
        );
        println!("[History]\n{}", self.history_to_string());
        println!("[Visited Urls]\n{:?}", self.visited_urls);

        // system prompt build
        let system_prompt = format!(
            r###"

                You are an automated web agent and data extraction engine.

                [Target Goal]
                {}

                [Rules]
                Before choosing an action, you MUST explain your rationale in detail within the 'reasoning' field.
                Respond ONLY with valid JSON. Do NOT use markdown code blocks or any extra text.

                [Strictly Allowed Actions Schema]
                'reasoning' field is mandatory for all actions.
                {}

                [Response Format]
                SINGLE JSON object matching this exact nested json shape:
                {{
                  "action": "<Action>",
                  "params": {{
                      "reasoning": "<Why you chose this action>",
                      ... <other exact action parameters 100% matching to schema>
                    }}
                  }}
            "###,
            self.system_task_prompt, action_guide,
        );

        // user prompt (current state prompt) build
        let elem = match &obs.focused_selectors {
            Some(schema) => lib::page::get_target_items(self.page, schema).await?,
            None => Vec::new(),
        };

        let focused_elem = self.focus_to_some_items(elem, 8).await?;

        let focused_elem_json = serde_json::to_string_pretty(&focused_elem)
            .unwrap_or_else(|_| format!("{:#?}", focused_elem));

        println!(
            "🔎 [DOM Elements Focused]:\n{}",
            focused_elem_json.chars().take(500).collect::<String>()
        );

        let current_user_prompt = format!(
            "
            [Your Action History] (Recent)
            {}

            [Your Current Route Location]
            {}

            [Current DOM elements]
            {}


            Choose the next Action schema in JSON.",
            self.history_to_string(),
            obs.current_url,
            focused_elem_json,
        );

        // llm request 포맷 생성
        let request = GenerationRequest::new(self.model.clone(), current_user_prompt)
            .system(system_prompt.to_string())
            .format(FormatType::Json);

        // llm 응답 올 때 까지 스피너
        let spinner = ProgressBar::new_spinner();

        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴"]),
        );

        spinner.set_message("LLM response wating...");
        spinner.enable_steady_tick(Duration::from_millis(100));

        // llm 요청 던짐
        let llm_response = self.ollama.generate(request).await?;
        let response_text = llm_response.response.trim();

        spinner.finish_with_message("완료!\n");

        // llm response 가져옴
        println!("Agent thinks the next action is: {}", response_text);

        let clean_json = crate::lib::page::parse_json_from_llm_response(response_text);

        let action: AgentAction = serde_json::from_str(&clean_json)?;

        // println!("\n🧠 [Agent Thinking]: {}", action.reasoning());
        // println!("\n🏃 [Agent Action]: {:?}", action);

        // println!("Enter를 누르면 계속 진행합니다...");

        // let mut input = String::new();
        // io::stdin().read_line(&mut input).unwrap();

        // println!("계속 진행합니다!");

        Ok(action)
    }

    /// 간소화된 DOM 트리를 바탕으로 추출에 필요한 CSS Selector 스키마를 LLM으로부터 취득
    pub async fn ask_important_selectors(
        &self,
        simplified_dom: &str,
    ) -> Result<SelectorSchema, DynError> {
        println!(
            "🪐 [Selector Extractor] LLM Loading... simplified_dom length: {}",
            simplified_dom.len()
        );

        // CSS selector prompt
        let system_prompt = format!(
            r#"
            You are a precise CSS Selector extraction engine.
            Task: Find important element selectors from HTML DOM tree.
            Note: "{}"
            Output format: Provide ONLY a valid JSON object matching this exact shape:
            {{
              "fields": {{
                ... <field selectors, necessary for the Task>
              }}
            }}
            [CSS Selector Rules]
            - Keep selectors as short and clean as possible, while ensuring each selector uniquely identifies its target item without ambiguity.
            - Prefer direct class/id or short structural jumps (e.g., `article a`, `a.group`, `.thumbnail-wrapper img`).
            - ONLY return standard CSS selectors compatible with `document.querySelector`.
            - NEVER use non-standard pseudo-elements (`::text`, `::attr(...)`, `:contains(...)`).
            - DO NOT use Tailwind style classes (e.g., `max-[60px]`).
            "#,
            self.page_observation_system_prompt.trim()
        );

        let user_prompt = format!(
            r#"
            HTML DOM tree:
                ---
                {}
                ---
            "#,
            simplified_dom.trim(),
        );

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴"]),
        );
        spinner.set_message("Waiting LLM to finish DOM element selection...\n");
        spinner.enable_steady_tick(Duration::from_millis(100));

        let request = GenerationRequest::new(self.model.clone(), user_prompt)
            .system(system_prompt)
            .format(FormatType::Json);
        let llm_response = self.ollama.generate(request).await?;
        let cleaned_json = lib::page::parse_json_from_llm_response(llm_response.response.trim());

        spinner.finish_with_message("Done!\n");

        // 2. JSON 파싱 (serde_json)
        let parsed_schema: SelectorSchema = serde_json::from_str(&cleaned_json).map_err(|e| {
            format!(
                "JSON Deserialization failed: {} | Failed for this data: {}",
                e, cleaned_json
            )
        })?;

        // 3. 실제 DOM 수집 검증
        let items = lib::page::get_target_items(self.page, &parsed_schema).await?;
        if items.is_empty() {
            // return Err(format!("Schema '{:?}' returned 0 items from DOM", parsed_schema).into());
            // just print error and pass emprty result
            println!("Schema '{}' returned 0 items from DOM", parsed_schema);
            return Ok(SelectorSchema::default());
        }

        Ok(parsed_schema)
    }

    // selector로 선택된 Targetitems 의 목록이 너무 길면 llm이 환각을 일으키므로 5~6개 정도의 후보군으로 줄여주는 메서드
    pub async fn focus_to_some_items(
        &self,
        items: Vec<TargetItem>,
        max_items: usize,
    ) -> DynResult<Vec<TargetItem>> {
        println!(
            "🟠 Filtering valid items and focusing to max {} items from {} items...",
            max_items,
            items.len(),
        );

        // 1. 유효 좌표(화면 내부)를 가진 항목만 1차 좌표 샌드박싱
        let valid_items: Vec<TargetItem> = items
            .iter()
            .filter(|item| item.center_x > 1.0 && item.center_y > 1.0)
            .cloned()
            .collect();

        if valid_items.is_empty() {
            return Ok(Vec::new());
        }

        // 방문한 적 있는 item제외하는 2차 필터
        // 1. 유효 좌표 및 중복 방문 URL 제거 (코드 레벨 1차 필터링)
        let valid_unvisited_items: Vec<TargetItem> = valid_items
            .into_iter()
            .filter(|item| {
                // 좌표 검증
                let is_valid_coord = item.center_x > 1.0 && item.center_y > 1.0;

                // URL 방문 여부 검증
                let is_not_visited = match &item.target_url {
                    Some(url) => !self.is_visited(url),
                    None => true, // URL 정보가 없으면 유지
                };

                // 외부 도메인으로 튕겨 나가는 것을 막기 위해 도메인 필터링 조건을 추가해 주는 것이 더 완벽합니다.
                let is_same_domain = match &item.target_url {
                    Some(url) => {
                        url.contains(&normalize_url(&self.target_webpage)) || url.starts_with('/')
                    }
                    None => true,
                };
                is_valid_coord && is_not_visited && is_same_domain
            })
            .take(30) // 최대 30개만 프롬프트에 입력!
            .collect();

        // 이미 별로 안 남았으면 리턴
        if valid_unvisited_items.len() <= max_items {
            return Ok(valid_unvisited_items);
        }

        // 많으면 llm한테 물어봄
        let items_json = serde_json::to_string_pretty(&valid_unvisited_items)
            .unwrap_or_else(|_| format!("{:#?}", valid_unvisited_items));

        let system_prompt = "You are a DOM observation item filtering engine.";

        let prompt = format!(
            r"#
            [Goal]
            {}

            [Action History]
            {}


            [Current DOM Observation Elements]
            {}


            Select at most {} items from Candidate DOM Observation Elements for the next action step.
            Output strictly as JSON. Do not add text or markdown.
            Do Not create unexisting fields or elements. Only return exisiting elements precisely, as it is.
            #",
            self.system_task_prompt,
            self.history_to_string(),
            items_json,
            max_items
        );

        // 3. LLM 호출
        let request = GenerationRequest::new(self.model.clone(), prompt)
            .system(system_prompt)
            .format(FormatType::Json);

        // llm 응답 올 때 까지 스피너
        let spinner = ProgressBar::new_spinner();

        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴"]),
        );

        spinner.set_message("LLM 응답 대기 중");
        spinner.enable_steady_tick(Duration::from_millis(100));

        let response = match self.ollama.generate(request).await {
            Ok(resp) => resp,
            Err(e) => {
                println!(
                    "⚠️ [Focus LLM Error] LLM 필터링 실패, Fallback 적용하여 앞쪽에 있는 아이템들 선택: {}",
                    e
                );
                return Ok(valid_unvisited_items.into_iter().take(max_items).collect());
            }
        };

        spinner.finish_with_message("완료!\n");

        // 4. 응답 파싱 및 안전 Fallback
        let cleaned_json = lib::page::parse_json_from_llm_response(response.response.trim());

        println!(
            "🎯 [Selector Focus Engine] LLM 응답 파싱 완료: {}",
            cleaned_json
        );

        let chosen_items_vec = serde_json::from_str::<TargetItemWrapper>(&cleaned_json)
            .map(|parsed| parsed.into_vec())
            .unwrap_or_default();

        if !chosen_items_vec.is_empty() {
            println!(
                "🎯 [Selector Focus Engine] 처음 {}개 중 최종 {}개 유효 요소를 선택함",
                items.len(),
                chosen_items_vec.len()
            );
            Ok(chosen_items_vec.into_iter().take(max_items).collect())
        } else {
            println!("⚠️ [Focus LLM Warning] LLM 응답 파싱 실패 또는 빈 배열. 유효 Fallback 적용.");
            Ok(valid_unvisited_items.into_iter().take(max_items).collect())
        }
    }

    /// TargetItem 정보를 바탕으로 target_url 기반 exact anchor 클릭을 시도하고,
    /// 실패 시 (center_x, center_y) 좌표로 CDP Native 마우스 이동 및 클릭을 수행합니다.
    pub async fn click_element_smart(
        &self,
        item: &TargetItem,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // // 💡 1. target_url이 명시되어 있다면(Some이라면) DOM에서 해당 <a> 태그를 찾아 exact click 우선 시도
        // if let Some(url) = &item.target_url {
        //     let relative_path = if let Ok(parsed) = url::Url::parse(url) {
        //         parsed.path().to_string()
        //     } else {
        //         url.to_string()
        //     };

        //     // 시각화 요소 주입 (실패하더라도 클릭 본체 로직에 영향을 주지 않도록 에러 무시)
        //     // ((현재 다른 위치에서 하는 중))
        //     // if let Err(e) = lib::page::inject_visual_cursor(self.page).await {
        //     //     eprintln!(
        //     //         "⚠️ [Visual Cursor Warning] 커서 주입 실패 (계속 진행): {}",
        //     //         e
        //     //     );
        //     // }

        //     let js_click_code = format!(
        //         r#"
        //             (() => {{
        //                 const targetUrl = "{}";
        //                 const relPath = "{}";
        //                 const selector = "{}";

        //                 // exact href 매칭 <a> 태그 검색
        //                 let targetA = document.querySelector(`a[href="${{targetUrl}}"], a[href="${{relPath}}"]`);

        //                 if (!targetA && selector) {{
        //                     const el = document.querySelector(selector);
        //                     if (el) targetA = el.tagName === 'A' ? el : el.closest('a');
        //                 }}

        //                 if (targetA) {{
        //                     targetA.scrollIntoView({{ block: 'center', inline: 'center' }});
        //                     targetA.click();
        //                     return true;
        //                 }}
        //                 return false;
        //             }})()
        //             "#,
        //         url, relative_path, item.selector
        //     );

        //     if let Ok(eval_res) = self.page.evaluate(&js_click_code).await {
        //         // 💡 String 결과값을 직접 "true"와 비교
        //         if eval_res.trim() == "true" {
        //             println!(
        //                 "🎯 [Exact Anchor Click] target_url('{}') 기반 exact DOM 클릭 성공",
        //                 url
        //             );
        //             return Ok(format!("Exact Anchor Clicked for URL '{}'", url));
        //         }
        //     }
        // }

        // 💡 1. target_url이 존재하면 href 속성 셀렉터(a[href*='...'])를 직접 생성하여 exact click 시도
        if let Some(ref url) = item.target_url {
            let relative_path = if let Ok(parsed) = url::Url::parse(url) {
                parsed.path().to_string()
            } else {
                url.to_string()
            };

            // target_url 경로를 이용한 exact attribute selector 구성
            let href_selector = format!("a[href*='{}']", relative_path);

            let js_click_code = format!(
                r#"
                        (() => {{
                            const targetUrl = "{}";
                            const relPath = "{}";
                            const exactHrefSelector = "{}";

                            // 1순위: a[href*='/i/xxx'] Exact Attribute Selector로 핀포인트 조화
                            let targetA = document.querySelector(exactHrefSelector);

                            // 2순위: 상대/절대 경로 직접 비교 Fallback
                            if (!targetA) {{
                                const anchors = Array.from(document.querySelectorAll('a'));
                                targetA = anchors.find(a => {{
                                    const href = a.getAttribute('href');
                                    return href && (href === targetUrl || href === relPath || a.href === targetUrl);
                                }});
                            }}

                            if (targetA) {{
                                targetA.scrollIntoView({{ block: 'center', inline: 'center' }});

                                const evt = new MouseEvent('click', {{ bubbles: true, cancelable: true, view: window }});
                                targetA.dispatchEvent(evt);
                                targetA.click();

                                return "true";
                            }}
                            return "false";
                        }})()
                        "#,
                url, relative_path, href_selector
            );

            if let Ok(eval_res) = self.page.evaluate(&js_click_code).await {
                let clean_res = eval_res.trim().trim_matches('"');
                if clean_res == "true" {
                    println!(
                        "🎯 [Exact Anchor Click] '{}' (URL: {}) 기반 exact DOM 클릭 성공",
                        href_selector, url
                    );
                    return Ok(format!("Exact Anchor Clicked for URL '{}'", url));
                }
            }
        }

        // 💡 2. target_url 클릭 실패 또는 URL이 없는 경우 이 블록으로 옴
        // - 좌표(center_x, center_y) 기반 하드웨어 마우스 클릭
        println!(
            "🎯 [Coordinate click] x, y : ({:.2}, {:.2}) \n",
            item.center_x, item.center_y
        );

        self.page
            .click_at_point(item.center_x, item.center_y)
            .await?;

        Ok(format!(
            "🎯 Clicked at ({:.2}, {:.2}) for selector '{}'",
            item.center_x, item.center_y, item.selector
        ))
    }

    /// 결정된 Action을 수행하는 헬퍼 메서드 (Tool Calling과 비슷함)
    pub async fn handle_action(
        &mut self,
        action: &AgentAction,
        obs: &AgentObservation,
    ) -> DynResult<(LoopControl, String)> {
        let current_url = &obs.current_url;

        match action {
            // AgentAction::ExtractData { .. } => {
            //     // 추출 전 기존 데이터는 잊어버리도록 함
            //     self.clean_history_data();

            //     println!("[Action] 데이터 추출 실행:");
            //     let history_formatter =
            //         HistoryFormatter::new("extract", "ExtractData", current_url);

            //     let (extracted_data, is_cached) =
            //         match self.try_extract_with_cached_selectors(current_url).await {
            //             Ok(Some(data)) => (data, true),
            //             _ => {
            //                 let selectors = self.extract_selectors(&obs.simplified_dom).await?;
            //                 let data = crate::lib::page::extract_data_with_selectors(
            //                     self.page, &selectors,
            //                 )
            //                 .await?;
            //                 self.selectors_cache.insert(current_url.clone(), selectors);
            //                 (data, false)
            //             }
            //         };

            //     println!("추출된 데이터: {:?}", extracted_data);
            //     // 파일로 저장
            //     self.save_extracted_data(current_url, &extracted_data)
            //         .await?;

            //     self.last_seen_page_data = Some(extracted_data.clone());

            //     let status = if is_cached {
            //         "success (cached)"
            //     } else {
            //         "success (new)"
            //     };
            //     let msg = history_formatter.format(
            //         status,
            //         "Data extraction successful",
            //         &extracted_data.to_string(),
            //     );

            //     Ok((LoopControl::Continue, msg))
            // }
            AgentAction::ReadFile { filename, .. } => {
                // 추출 전 기존 데이터는 잊어버리도록 함
                self.clean_history_data();

                println!("[Action] 파일 읽기 실행: {}", filename);
                let history_formatter = HistoryFormatter::new("read", "ReadFile", current_url);

                if !Path::new(filename).exists() {
                    let msg = history_formatter.format(
                        "failed: file not found",
                        "File check failed",
                        &format!("File not found: '{}'", filename),
                    );
                    return Ok((LoopControl::Continue, msg));
                }

                match tokio::fs::read_to_string(filename).await {
                    Ok(content) => {
                        println!("파일 읽기 완료 ({} bytes)", content.len());
                        let msg =
                            history_formatter.format("success", "File read successful", &content);
                        Ok((LoopControl::Continue, msg))
                    }
                    Err(e) => {
                        let msg = history_formatter.format(
                            "failed",
                            "File read error",
                            &format!("Failed to read file '{}': {}", filename, e),
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                }
            }

            AgentAction::WriteFile {
                filename, content, ..
            } => {
                println!("[Action] 파일 쓰기 실행: {}", filename);
                let history_formatter = HistoryFormatter::new("write", "WriteFile", current_url);

                match tokio::fs::write(filename, content).await {
                    Ok(_) => {
                        let payload = format!(
                            "Successfully wrote {} bytes to '{}'",
                            content.len(),
                            filename
                        );
                        let msg =
                            history_formatter.format("success", "File write successful", &payload);
                        Ok((LoopControl::Continue, msg))
                    }
                    Err(e) => {
                        let payload = format!("Failed to write to file '{}': {}", filename, e);
                        let msg = history_formatter.format("failed", "File write failed", &payload);
                        Ok((LoopControl::Continue, msg))
                    }
                }
            }

            AgentAction::ScrollPage { dy, .. } => {
                println!("[Action] 페이지 스크롤: dy={}", dy);
                let history_formatter = HistoryFormatter::new("scroll", "ScrollPage", current_url);

                let eval_result = self
                    .page
                    .evaluate(&format!("window.scrollBy(0, {});", dy))
                    .await;
                tokio::time::sleep(Duration::from_secs(1)).await;

                match eval_result {
                    Ok(_) => {
                        let msg = history_formatter.format(
                            "success",
                            "Page scrolled successfully",
                            &format!("Scrolled page by {}px", dy),
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                    Err(e) => {
                        let msg = history_formatter.format(
                            "failed",
                            "Page scroll failed",
                            &format!("Failed to scroll page by {}px: {}", dy, e),
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                }
            }

            AgentAction::ClickElement {
                selector,
                target_url,
                x,
                y,
                reasoning: _,
            } => {
                let history_formatter = HistoryFormatter::new("click", "ClickElement", current_url);

                // // 1. obs.target_elements에서 matching 되는 TargetItem 룩업 (없으면 임시 TargetItem 생성)
                // let matched_item =
                //     TargetItem::new(selector.to_string(), Some(target_url.clone()), x, y);
                // 룩업을 하려면 id같은걸 element에 커스텀으로 추가해서 룩업하게 할까? 지금은 일단 받은 파라미터 값 그대로를 쓰기로 한다.

                // target build
                let target = TargetItem::new(
                    selector.to_string(),
                    Some(target_url.clone()),
                    x.unwrap_or(0.0),
                    y.unwrap_or(0.0),
                );

                // 3. TargetItem 참조로 click_element_smart 호출
                println!("[Action] 클릭 시도: {:?}", &target);

                // 2. TargetItem 참조로 click_element_smart 호출
                match self.click_element_smart(&target).await {
                    Ok(detail_msg) => {
                        tokio::time::sleep(Duration::from_millis(1500)).await;

                        let new_url = self
                            .page
                            .current_url()
                            .await
                            .unwrap_or_else(|_| current_url.clone());

                        println!("🔗 [Click Success] new updated URL: {}", new_url);

                        // 방문 기록 저장
                        self.mark_visited(&new_url);
                        if let Some(ref url) = target.target_url {
                            self.mark_visited(url); // 이러면 2개씩 생기는 거 아닌가? ;
                        }

                        let msg = history_formatter.format(
                            "success",
                            "Successfully clicked target element",
                            &detail_msg,
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                    Err(e) => {
                        eprintln!(
                            "Click Execution Error for selector '{}': {}",
                            &target.selector, e
                        );
                        let msg = history_formatter.format(
                            "failed",
                            "Click element failed",
                            &format!(
                                "Failed to click element with selector '{}': {}",
                                &target.selector, e
                            ),
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                }
            }

            AgentAction::BackPage { .. } => {
                println!("[Action] 이전 페이지 이동 (BackPage)");
                let history_formatter = HistoryFormatter::new("back", "BackPage", current_url);

                match self.page.go_back().await {
                    Ok(_) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let msg = history_formatter.format(
                            "success",
                            "Navigated back successfully",
                            "Successfully navigated back to previous page",
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                    Err(e) => {
                        let msg = history_formatter.format(
                            "failed",
                            "Back navigation failed",
                            &format!("Failed to navigate back: {}", e),
                        );
                        Ok((LoopControl::Continue, msg))
                    }
                }
            }

            AgentAction::Finish { reason, .. } => {
                println!("[Action] 작업 완료: {}", reason);
                let history_formatter = HistoryFormatter::new("finish", "Finish", current_url);
                let msg = history_formatter.format(
                    "success",
                    "Task completed",
                    &format!("Task finished with reason: {}", reason),
                );

                Ok((LoopControl::Finish, msg))
            }
        }
    }

    pub async fn observe_current_page(&mut self) -> DynResult<AgentObservation> {
        // 1. Observe: 스텝 시작 시 DOM, simpleified_dom 관측
        // println!("📍 현재 위치 (debug): {}", self.page.current_url().await?);

        // 💡 관측 전 최대 5초간 페이지 로드 완료 대기
        lib::page::wait_for_page_load(self.page, 5).await?;
        let agent_observation = self.capture_observation().await?;

        // 2. 관찰 시점의 Simplified DOM 즉시 저장
        if let Err(e) = self
            .save_simplified_dom(
                &agent_observation.current_url,
                &agent_observation.simplified_dom,
            )
            .await
        {
            eprintln!("⚠️ [Run Observation DOM Save Warning] {}", e);
        }

        Ok(agent_observation)
    }

    /// 메인 루프 실행 메서드
    pub async fn run(&mut self) -> DynResult<()> {
        let mut step = 1;
        loop {
            println!("\n=== Agent Step {} ===", step);

            let mut agent_observation = None;
            let max_try = 3;
            for attempt in 1..=max_try {
                match self.observe_current_page().await {
                    Ok(observation) => {
                        agent_observation = Some(observation);
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "⚠️ [Observe Error] 페이지 관측 실패: {} ({}/{} 재시도 중...)",
                            e, attempt, max_try
                        );
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            }

            // 💡 Shadowing: 변수 이름은 그대로 유지하면서 Option을 완전히 제거!
            let agent_observation = match agent_observation {
                Some(obs) => {
                    println!("✅ [Observeation Success]");
                    obs
                }
                None => {
                    eprintln!("💥 페이지 관측(observeCurrentPage) 3회 연속 실패로 작업 종료");
                    break;
                }
            };

            let current_url = &agent_observation.current_url;
            println!("📍 current url: {}", current_url);
            self.mark_visited(&current_url);
            if self.visited_urls.len() >= 50 {
                println!("✅ [Visited Urls] Length: {}", self.visited_urls.len());
                println!("Exiting due to visited urls length 50");
                break;
            }

            // 4. Think: LLM 추론 (최대 3회 재시도)
            let mut action_result = None;
            for attempt in 1..=3 {
                match self.ask_step(&agent_observation).await {
                    Ok(action) => {
                        action_result = Some(action);
                        break;
                    }
                    Err(e) => {
                        println!("LLM 요청 실패 ({}/3): {}", attempt, e);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }

            let action = match action_result {
                Some(act) => act,
                None => {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "LLM 요청 3회 연속 실패",
                    )));
                }
            };

            // 소유권 이동 전, 히스토리에 기록할 정보를 미리 준비하거나 참조(&action)로 전달
            let action_name = action.name().to_string();
            let reasoning = action.reasoning().to_string();

            // 3. Act: 행동 수행 및 결과 메시지 수집
            let (control, result_summary) =
                match self.handle_action(&action, &agent_observation).await {
                    Ok(res) => res,
                    Err(e) => (
                        LoopControl::Continue,
                        format!("Critical action execution error: {}", e),
                    ),
                };

            // Extract data from result_summary if it exists
            let data = if result_summary.contains("Data: ") {
                Some(result_summary.split("Data: ").nth(1).unwrap().to_string())
            } else {
                None
            };

            // 4. Record: 행동 명칭, 이유, 실행 결과를 히스토리에 기록
            let history_entry = ActionHistory {
                step,
                action_name,
                reasoning,
                status: result_summary,
                data,
            };
            self.history.push(history_entry);
            self.trim_history();

            // 5. Termination Check: 작업 종료 판단
            if matches!(control, LoopControl::Finish) {
                println!("에이전트 작업을 성공적으로 마쳤습니다.");
                break;
            }

            step += 1;
        }
        Ok(())
    }
}
