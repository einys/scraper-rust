use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "params")]
pub enum AgentAction {
    WriteFile {
        reasoning: String, // 👈 생각 과정 추가
        filename: String,
        content: String,
    },
    ScrollPage {
        reasoning: String, // 👈 생각 과정 추가
        dy: i32,
    },
    #[serde(alias = "click", alias = "click_element", alias = "Click")]
    ClickElement {
        reasoning: String, // 👈 생각 과정 추가
        selector: String,
    },
    Finish {
        reasoning: String, // 👈 생각 과정 추가
        reason: String,
    },
    ExtractData {
        reasoning: String,
        selectors: Vec<String>,
    },
}
impl AgentAction {
    // reasoning 필드만 쉽게 뽑아내는 헬퍼 메서드
    pub fn reasoning(&self) -> &str {
        match self {
            AgentAction::WriteFile { reasoning, .. } => reasoning,
            AgentAction::ScrollPage { reasoning, .. } => reasoning,
            AgentAction::ClickElement { reasoning, .. } => reasoning,
            AgentAction::Finish { reasoning, .. } => reasoning,
            AgentAction::ExtractData { reasoning, .. } => reasoning,
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

pub struct WebAgent<'a, P: PageTrait> {
    ollama: Ollama,
    model: String,
    page: &'a P,
    task_goal: String,
    target_description: String,
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
        }
    }

    /// 메인 루프 실행 메서드
    pub async fn run(&self) -> DynResult<()> {
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
                    Err(_) => {
                        println!("LLM 요청 실패 ({}/3). 다시 시도합니다.", attempt);
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

            // 3. Act: 행동 수행
            match self.handle_action(action, &obs).await? {
                LoopControl::Finish => {
                    println!("에이전트 작업을 성공적으로 마쳤습니다.");
                    break;
                }
                LoopControl::Continue => {}
            }

            step += 1;
        }
        Ok(())
    }

    /// LLM에 다음 단계를 추론하도록 요청하는 헬퍼 메서드
    async fn ask_step(&self, current_dom: &str) -> DynResult<AgentAction> {
        let system_prompt = r###"
You are an automated web agent and data extraction assistant.
Before choosing an action, you MUST explain your rationale in detail within the 'reasoning' field.
Respond ONLY with a valid JSON object matching one of the schemas below. Do NOT include any extra text or Markdown formatting.

1. Write File: {"action": "WriteFile", "params": {"reasoning": "...", "filename": "output.json", "content": "..."}}
2. Scroll Page: {"action": "ScrollPage", "params": {"reasoning": "...", "dy": 1500}}
3. Click Element: {"action": "ClickElement", "params": {"reasoning": "...", "selector": "#btn"}}
4. Finish Task: {"action": "Finish", "params": {"reasoning": "...", "reason": "Task completed"}}
5. Extract Data: {"action": "ExtractData", "params": {"reasoning": "...", "selectors": ["..."]}}
"###;

        let prompt = format!(
            "Target Goal: {}\n\nCurrent DOM State:\n{}\n\nDetermine the next action:",
            self.task_goal, current_dom
        );

        let request = GenerationRequest::new(self.model.clone(), prompt)
            .system(system_prompt.to_string())
            .format(FormatType::Json);

        let llm_response = self.ollama.generate(request).await?;
        let response_text = llm_response.response.trim();
        let clean_json = crate::lib::page::parse_json_from_llm_response(response_text);

        let action: AgentAction = serde_json::from_str(&clean_json)?;

        println!("\n🧠 [Agent Thinking]: {}", action.reasoning());
        println!("🎯 [Agent Action]: {:?}", action);

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

        let prompt = format!(
            r#"Below is a simplified, REPRESENTATIVE HTML DOM tree:
    ---
    {}
    ---
    Task: Find the CSS Selectors to extract a list of items for: "{}"
    Output format: Provide a concise JSON object containing:
    - "container": "topmost card wrapper selector",
    - "fields": JSON object mapping field names to their relative CSS selectors inside the container.
    IMPORTANT RULES: Output MUST be the SIMPLEST, SHORTEST, standard CSS selectors ONLY. Provide ONLY valid JSON."#,
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

    /// 결정된 Action을 수행하는 헬퍼 메서드
    async fn handle_action(
        &self,
        action: AgentAction,
        obs: &AgentObservation,
    ) -> DynResult<LoopControl> {
        match action {
            AgentAction::WriteFile {
                filename, content, ..
            } => {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&filename)
                    .await?;
                file.write_all(content.as_bytes()).await?;
                println!("[Action] 파일 내용 추가 완료: {}", filename);
            }
            AgentAction::ScrollPage { dy, .. } => {
                println!("[Action] 페이지 스크롤: dy={}", dy);
                self.page
                    .evaluate(&format!("window.scrollBy(0, {});", dy))
                    .await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            AgentAction::ClickElement { selector, .. } => {
                println!("[Action] 요소 클릭: {}", selector);
                let element = self.page.find_element(&selector).await?;
                element.click().await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            AgentAction::Finish { reason, .. } => {
                println!("[Action] 에이전트 종료 요청: {}", reason);
                return Ok(LoopControl::Finish);
            }
            AgentAction::ExtractData { selectors: _, .. } => {
                println!("[Action] 데이터 추출 실행");

                // 💡 WebAgent 내부 메서드로 깔끔하게 호출
                let selectors = self.extract_selectors(&obs.simplified_dom).await?;
                let extracted_data =
                    crate::lib::page::extract_data_with_selectors(self.page, &selectors).await?;

                println!("추출된 데이터: {:?}", extracted_data);
            }
        }
        Ok(LoopControl::Continue)
    }
}
