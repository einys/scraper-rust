use schemars::schema_for;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::lib::dyn_result::DynError;
use crate::lib::dyn_result::DynResult;
use crate::lib::page::SelectorSchema;
use crate::PageTrait;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::Ollama;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", content = "params")]
pub enum AgentAction {
    /// 파일을 읽어옵니다.
    ReadFile { reasoning: String, filename: String },
    /// 파일에 텍스트 내용을 쓰거나 추가합니다.
    WriteFile {
        reasoning: String, // 👈 생각 과정 추가
        filename: String,
        content: String,
    },
    /// 수직으로 페이지를 스크롤합니다.
    ScrollPage {
        reasoning: String, // 👈 생각 과정 추가
        dy: i32,
    },
    /// 특정 CSS 셀렉터 요소를 클릭합니다.
    #[serde(alias = "click", alias = "click_element", alias = "Click")]
    ClickElement {
        reasoning: String, // 👈 생각 과정 추가
        selector: String,
    },
    /// 모든 수집 목표를 완료하고 에이전트를 종료합니다.
    Finish {
        reasoning: String, // 👈 생각 과정 추가
        reason: String,
    },
    /// DOM에서 원하는 셀렉터 목록으로 데이터를 추출합니다.
    ExtractData {
        reasoning: String,
        selectors: Vec<String>,
    },
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
            AgentAction::ExtractData { reasoning, .. } => reasoning,
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
            AgentAction::ExtractData { .. } => "ExtractData",
            AgentAction::BackPage { .. } => "BackPage",
        }
    }

    /// 히스토리에 기록할 주요 파라미터 요약 정보
    pub fn to_string(&self) -> String {
        match self {
            AgentAction::ReadFile { filename, .. } => format!("ReadFile({})", filename),
            AgentAction::WriteFile { filename, .. } => format!("WriteFile({})", filename),
            AgentAction::ScrollPage { dy, .. } => format!("ScrollPage(dy={})", dy),
            AgentAction::ClickElement { selector, .. } => format!("ClickElement({})", selector),
            AgentAction::Finish { reason, .. } => format!("Finish({})", reason),
            AgentAction::ExtractData { selectors, .. } => {
                format!("ExtractData(count={})", selectors.len())
            }
            AgentAction::BackPage { .. } => "BackPage".to_string(),
        }
    }
}

pub struct AgentObservation {
    pub simplified_dom: String,
    pub html_content: String,
    // 필요 시 추가 가능 (예: pub current_url: String, pub screenshot: Vec<u8>)
}

impl AgentObservation {
    pub async fn capture<P: PageTrait>(page: &P) -> DynResult<Self> {
        let simplified_dom = crate::lib::page::get_simplified_dom_tree(page).await?;
        let html_content = page.content().await?;

        Ok(Self {
            simplified_dom,
            html_content,
        })
    }
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
    pub status: String, // 예: "Success", "Failed: Element not found"
}
impl ActionHistory {
    pub fn to_prompt_string(&self) -> String {
        format!(
            "Step {}: Action='{}' | Status='{}'\n  Reasoning: {}",
            self.step, self.action_name, self.status, self.reasoning
        )
    }
}

use std::collections::HashMap;

#[derive(Default)]
pub struct RouteSelectors {
    // Route pattern (e.g., "/i/[id]" or "list_page") -> Selector List
    pub cache: HashMap<String, Vec<String>>,
}

pub struct WebAgent<'a, P: PageTrait> {
    ollama: Ollama,
    model: String,
    page: &'a P,
    task_goal: String,
    target_description: String,
    // 💡 스텝별 행동 기록을 누적 보관하는 필드 추가
    history: Vec<ActionHistory>,
    max_history_depth: usize, // 토큰 폭발 방지를 위한 최근 기록 유지 개수
    pub selectors_cache: RouteSelectors,
}

impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// Agent 생성자
    pub fn new(
        ollama_host: &str,
        ollama_port: u16,
        model: &str,
        page: &'a P,
        task_goal: &str,
        target_description: &str,
    ) -> Self {
        let ollama = Ollama::builder()
            .host(ollama_host.to_string())
            .port(ollama_port)
            .build();

        Self {
            ollama,
            model: model.to_string(),
            page,
            task_goal: task_goal.to_string(),
            target_description: target_description.to_string(),
            history: Vec::new(),
            max_history_depth: 10, // 기본적으로 최근 10개의 이전 스텝을 저장합니다.
        }
    }

    /// 메인 루프 실행 메서드
    pub async fn run(&mut self) -> DynResult<()> {
        let mut step = 1;
        loop {
            println!("\n=== Agent Step {} ===", step);

            // 1. Observe: 스텝 시작 시 관측
            let obs = AgentObservation::capture(self.page).await?;

            // 2. Think: LLM 추론 (최대 3회 재시도)
            let mut action_result = None;
            for attempt in 1..=3 {
                match self.ask_step(&obs.simplified_dom).await {
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
            let (control, result_summary) = match self.handle_action(&action, &obs).await {
                Ok(res) => res,
                Err(e) => (
                    LoopControl::Continue,
                    format!("Critical action execution error: {}", e),
                ),
            };

            // 4. Record: 행동 명칭, 이유, 실행 결과를 히스토리에 기록
            let history_entry = ActionHistory {
                step,
                action_name,
                reasoning,
                status: result_summary,
            };
            self.history.push(history_entry);

            // 5. Termination Check: 작업 종료 판단
            if matches!(control, LoopControl::Finish) {
                println!("에이전트 작업을 성공적으로 마쳤습니다.");
                break;
            }

            step += 1;
        }
        Ok(())
    }

    /// schemars로 만든 스키마를 LLM 전달용으로 축소
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

                if let Some(props) = item.get("properties") {
                    let action_name = props
                        .get("action")
                        .and_then(|a| a.get("enum"))
                        .and_then(|e| e.get(0))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");

                    let mut params_fields = Vec::new();
                    if let Some(params_obj) = props.get("params").and_then(|p| p.get("properties"))
                    {
                        if let Some(map) = params_obj.as_object() {
                            for (field, _) in map {
                                params_fields.push(field.as_str());
                            }
                        }
                    }

                    // 축약된 형태 생성: - ActionName: 설명 (required params: field1, field2)
                    compact_docs.push_str(&format!(
                        "- Action: `{}`\n  Description: {}\n  Params: {}\n\n",
                        action_name,
                        description,
                        serde_json::to_string(&params_fields).unwrap_or_default()
                    ));
                }
            }
        }

        compact_docs
    }

    fn compact_history(&self) -> String {
        self.history
            .iter()
            .map(|h| h.to_prompt_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// LLM에 다음 단계를 추론하도록 요청하는 헬퍼 메서드
    async fn ask_step(&self, current_dom: &str) -> DynResult<AgentAction> {
        let action_guide = self.build_compact_action_guide();

        println!("LLM에 다음 단계를 추론하도록 요청합니다.");
        println!(
            "목록에 포함된 Actions List:\n{}...",
            action_guide.chars().take(50).collect::<String>()
        );
        println!("History:\n{}", self.compact_history());

        let system_prompt = format!(
            r###"
You are an automated web agent and data extraction assistant.
Before choosing an action, you MUST explain your rationale in detail within the 'reasoning' field.
Respond ONLY with a valid JSON object matching one of the schemas below. Do NOT include any extra text or Markdown formatting.

[Strictly Allowed Actions Schema]
{}


[Response Format]
Respond with a SINGLE JSON object matching this exact shape:
{{
  "action": "<ActionName>",
  "params": {{
    "reasoning": "<Why you chose this action>",
    ... <other action parameters>
  }}
}}

Do NOT wrap response in markdown code blocks. Do NOT output extra text.


[File Usage Guidelines]
You must choose the appropriate filename based on the current page URL route structure:

1. For List / Feed Pages (e.g., `cmsn.info/` or main feed):
   - Selector File: `selectors_list.json`
   - Data File: `items_list.json`

2. For Item Detail Pages (e.g., `cmsn.info/i/[id]` or any dynamic item route):
   - Selector File: `selectors_detail.json`
   - Data File: `items_detail.json`

[File Usage Rules]

- Write and Read data, selectors, in files. Reuse them for same route. Before trying to extract data, check if the data file already exists.
- When extracting selectors on a detail page (`/i/...`), save or read from `selectors_detail.json` so that all dynamic item routes reuse the exact same schema.
- When writing extracted item data, append to `items_detail.json` (or `items_list.json` depending on the page type).
- Do NOT generate unique filenames containing specific IDs (e.g., `items_AAAAA.json` is FORBIDDEN).

[Your Action History]
{}
"###,
            action_guide,
            self.compact_history()
        );

        let prompt = format!(
            "Target Goal: {}\n\nCurrent DOM State:\n{}\n\nDetermine the next action:",
            self.task_goal, current_dom
        );

        let request = GenerationRequest::new(self.model.clone(), prompt)
            .system(system_prompt.to_string())
            .format(FormatType::Json);

        let llm_response = self.ollama.generate(request).await?;
        let response_text = llm_response.response.trim();

        println!("Agent가 생각한 다음 action 초안: {}", response_text);

        let clean_json = crate::lib::page::parse_json_from_llm_response(response_text);

        let action: AgentAction = serde_json::from_str(&clean_json)?;

        println!("\n🧠 [Agent Thinking]: {}", action.reasoning());
        println!("\n🏃 [Agent Action]: {:?}", action);

        Ok(action)
    }

    /// 간소화된 DOM 트리를 바탕으로 추출에 필요한 CSS Selector 스키마를 LLM으로부터 취득
    pub async fn extract_selectors(
        &self,
        simplified_dom: &str,
    ) -> Result<SelectorSchema, DynError> {
        println!(
            "🪐 [Selector Extractor] LLM Loading... feed: simplified_dom length: {}",
            simplified_dom.len()
        );

        // CSS selector prompt
        let prompt = format!(
            r#"Below is a simplified, REPRESENTATIVE HTML DOM tree:
    ---
    {}
    ---
    Task: Find the CSS Selectors to extract a list of items for: "{}"
    Output format: Provide only a concise JSON object containing:
    - "container": "topmost card wrapper selector",
    - "fields": JSON object mapping field names to their relative CSS selectors inside the container.
    IMPORTANT RULES: Prefer SIMPLEST standard CSS selectors. Do not create non-existing selectors."#,
            simplified_dom.trim(),
            self.target_description.trim()
        );

        let max_retries = 3;
        let mut last_error: Option<DynError> = None;

        for attempt in 1..=max_retries {
            println!(
                "Attempt {}/{} requesting LLM for selectors...",
                attempt, max_retries
            );

            let request =
                GenerationRequest::new(self.model.clone(), prompt.clone()).format(FormatType::Json);

            match self.ollama.generate(request).await {
                Ok(ollama_response) => {
                    let raw_json = ollama_response.response.trim();
                    let cleaned_json = crate::lib::page::parse_json_from_llm_response(raw_json);

                    match serde_json::from_str::<SelectorSchema>(&cleaned_json) {
                        Ok(parsed_schema) => {
                            println!("Successfully parsed SelectorSchema!");
                            return Ok(parsed_schema);
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to parse JSON response on attempt {}: {}",
                                attempt, e
                            );
                            eprintln!("Raw LLM Output was: {}", raw_json);
                            last_error = Some(Box::new(e));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Ollama API call failed on attempt {}: {}", attempt, e);
                    last_error = Some(Box::new(e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Failed to extract valid selectors after retries".into()))
    }

    /// 결정된 Action을 수행하는 헬퍼 메서드 (Tool Calling과 비슷함)
    async fn handle_action(
        &self,
        action: &AgentAction,
        obs: &AgentObservation,
    ) -> DynResult<(LoopControl, String)> {
        match action {
            AgentAction::BackPage { reasoning } => {
                println!("[Action] Navigate back: {}", reasoning);
                match self.page.go_back().await {
                    Ok(_) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        Ok((
                            LoopControl::Continue,
                            "Successfully navigated back.".to_string(),
                        ))
                    }
                    Err(e) => Ok((
                        LoopControl::Continue,
                        format!("Failed to navigate back: {}", e),
                    )),
                }
            }

            AgentAction::ReadFile { filename, .. } => {
                println!("[Action] 파일 읽기 실행: {}", filename);
                let path = Path::new(filename);
                if !path.exists() {
                    return Ok((
                        LoopControl::Continue,
                        format!("File not found: '{}'", filename),
                    ));
                }

                match tokio::fs::read_to_string(filename).await {
                    Ok(content) => {
                        let truncated = if content.chars().count() > 50 {
                            format!("{}...", content.chars().take(47).collect::<String>())
                        } else {
                            content
                        };
                        Ok((
                            LoopControl::Continue,
                            format!(
                                "Successfully read file '{}'. Preview: '{}'",
                                filename, truncated
                            ),
                        ))
                    }
                    Err(e) => Ok((
                        LoopControl::Continue,
                        format!("Failed to read file '{}': {}", filename, e),
                    )),
                }
            }

            AgentAction::WriteFile {
                filename, content, ..
            } => {
                println!("[Action] 파일 쓰기 실행: {}", filename);
                match tokio::fs::write(filename, content).await {
                    Ok(_) => Ok((
                        LoopControl::Continue,
                        format!(
                            "Successfully wrote {} bytes to '{}'",
                            content.len(),
                            filename
                        ),
                    )),
                    Err(e) => Ok((
                        LoopControl::Continue,
                        format!("Failed to write to file '{}': {}", filename, e),
                    )),
                }
            }

            AgentAction::ScrollPage { dy, .. } => {
                println!("[Action] 페이지 스크롤: dy={}", dy);
                self.page
                    .evaluate(&format!("window.scrollBy(0, {});", dy))
                    .await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok((LoopControl::Continue, format!("Scrolled page by {}px", dy)))
            }

            AgentAction::ClickElement { selector, .. } => {
                println!("[Action] 요소 클릭 실행: {}", selector);
                // click 로직 예시
                match self.page.find_element(selector).await {
                    Ok(elem) => match elem.click().await {
                        Ok(_) => Ok((
                            LoopControl::Continue,
                            format!("Successfully clicked element '{}'", selector),
                        )),
                        Err(e) => Ok((
                            LoopControl::Continue,
                            format!("Failed to click element '{}': {}", selector, e),
                        )),
                    },
                    Err(_) => Ok((
                        LoopControl::Continue,
                        format!("Element not found with selector '{}'", selector),
                    )),
                }
            }

            AgentAction::ExtractData { selectors, .. } => {
                println!("[Action] 데이터 추출 실행: {:?}", selectors);

                // 💡 WebAgent 내부 메서드로 깔끔하게 호출
                let selectors = self.extract_selectors(&obs.simplified_dom).await?;
                let extracted_data =
                    crate::lib::page::extract_data_with_selectors(self.page, &selectors).await?;

                println!("추출된 데이터: {:?}", extracted_data);
                Ok((
                    LoopControl::Continue,
                    format!(
                        "Extracted data using {} selectors",
                        selectors.container.len() + selectors.fields.len()
                    ),
                ))
            }

            AgentAction::Finish { reason, .. } => {
                println!("[Action] 작업 완료: {}", reason);
                Ok((LoopControl::Finish, format!("Finished task: {}", reason)))
            }
        }
    }
}
