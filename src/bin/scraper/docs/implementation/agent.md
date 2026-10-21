1.3. Accessibility Tree + Indexing 상호작용 가능 요소만 1차원/인덱스로 압축 시각적 레이아웃 정보 손실 가능 (필요시 Screenshots 병행) ✅ 강력 추천

# 💡 추천하는 1.3 방식 (Interactive Indexing)

LLM에게 CSS Selector나 XPath를 생성하게 하면 틀릴 확률이 높습니다. 대신 상호작용 가능한 요소들에 0, 1, 2... 식의 고유 ID(Backend Node ID 또는 Index)를 부여하고 이를 넘겨주는 방식이 훨씬 안정적입니다.
Plaintext

[Accessibility/Simplified DOM Representation 예시]
[0] <button id="submit">로그인</button>
[1] <a href="/next">다음 페이지</a>
[2] <input placeholder="검색어 입력">
[3] <div role="article">게시글 본문 텍스트...</div>

에이전트는 selector 대신 {"action": "ClickElement", "params": {"index": 0}} 형태로 응답하도록 설계하면 파싱 실패와 환각률이 급격히 줄어듭니다. 2. 작성하신 Rust 스케치 코드 피드백 & 개선점

현재 스케치는 매우 잘 작성되어 있지만, 실제 동작 시 발생할 몇 가지 빈틈을 메우면 훨씬 탄탄해집니다.
① Enum Tagging 정제 (AgentAction 구조)

serde로 JSON을 바로 파싱할 때, 현재 프롬프트 구조대로라면 untagged나 adjacently tagged enum 형태가 됩니다. Rust의 Enum 정의와 프롬프트의 JSON 구조가 일치하는지 체크가 필요합니다.
Rust

// 추천하는 Serde Enum 구조 (Adjacent Tagging)
#[derive(Debug, Deserialize)]
#[serde(tag = "action", content = "params")]
pub enum AgentAction {
WriteFile { reasoning: String, filename: String, content: String },
ScrollPage { reasoning: String, dy: i32 },
ClickElement { reasoning: String, selector: String }, // 또는 index: usize
Finish { reasoning: String, reason: String },
}

② JSON 정제 로직의 한계

starts_with("```") 방식은 LLM이 JSON 앞뒤로 "여기 행동을 결정했습니다:" 같은 사족을 붙일 경우 파싱 에러가 납니다. 첫 번째 {와 마지막 }를 찾는 정규식이나 인덱스 슬라이싱이 훨씬 안전합니다.
Rust

fn extract_json(raw: &str) -> &str {
if let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) {
if start < end {
return &raw[start..=end];
}
}
raw.trim()
}

③ Prompt 내 Target Goal 명시 부족

현재 프롬프트는 "다음으로 수행할 행동은?"만 묻고 있습니다. 에이전트가 무슨 목적으로 페이지를 긁고 있는지 user_task / goal (예: "상품 목록에서 가격 정보를 추출해줘") 정보가 프롬프트에 들어가야 reasoning을 제대로 수행합니다.

---

다음과 같이 설계를 변경할 거야. 지금의 코드의 주석을 전부 그대로 유지하고, 문장도 그대로 쓰면서 리팩토링만 해 줘.

`Agent`를 클래스(Rust에서는 `struct` 및 `impl` 블록)로 선언하고, 내부 필드에 `Ollama` 클라이언트, 모델명, 브라우저 페이지, 목표(Goal) 등을 캡슐화해 두는 방식은 매우 훌륭하고 정석적인 설계입니다.

이렇게 하면 `agent_loop`나 `handle_action`에 매번 6~7개씩 인자를 길게 넘길 필요 없이, **`agent.run().await?`** 한 줄로 깔끔하게 실행할 수 있습니다.

---

### `WebAgent` 구조체 설계 예시

```rust
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use async_trait::async_trait;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::Ollama;

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
        let clean_json = lib::page::parse_json_from_llm_response(response_text);

        let action: AgentAction = serde_json::from_str(&clean_json)?;

        println!("\n🧠 [Agent Thinking]: {}", action.reasoning());
        println!("🎯 [Agent Action]: {:?}", action);

        Ok(action)
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
                let selectors = lib::page::extract_selectors_via_local_llm(
                    &obs.simplified_dom,
                    &self.target_description,
                    None,
                    Some(self.model.clone()),
                    &self.ollama,
                )
                .await?;
                let extracted_data =
                    lib::page::extract_data_with_selectors(self.page, selectors).await?;
                println!("추출된 데이터: {:?}", extracted_data);
            }
        }
        Ok(LoopControl::Continue)
    }
}

```

---

### `main` 함수 사용부

`main` 구문이 대폭 단순해지며 읽기 훨씬 쉬워집니다.

```rust
#[tokio::main]
async fn main() -> DynResult<()> {
    // 1. 브라우저 생성 및 이동
    let (browser, mut handler) = Browser::launch(config).await?;
    tokio::spawn(async move { /* 이벤트 루프 처리 */ });

    let raw_page = browser.new_page("https://cmsn.info").await?;
    raw_page.wait_for_navigation().await?;

    let page_adapter = ChromiumPageAdapter::new(raw_page);

    // 2. WebAgent 객체 생성 (Ollama 설정, 모델명, 목표를 캡슐화)
    let agent = WebAgent::new(
        "http://localhost",
        31434,
        "qwen3:14b",
        &page_adapter,
        "collect 50 items and details",
        "From the list of items, extract: content, username, href, img_src",
    );

    // 3. 실행
    agent.run().await?;

    Ok(())
}

```

이 구조로 묶어두면 함수 인자 전달이 깔끔해질 뿐만 아니라, 나중에 다른 모델이나 다른 설정의 에이전트를 여러 개 띄워 테스트할 때도 객체만 새로 생성하면 되므로 유지보수성이 크게 향상됩니다.

---

### 1. `ActionHistory` 구조체 정의

각 스텝에서 수행했던 사고 과정(Reasoning)과 최종 행동(Action 요약)을 기록할 단위를 선언합니다.

```rust
#[derive(Debug, Clone)]
pub struct ActionHistory {
    pub step: usize,
    pub reasoning: String,
    pub action_summary: String,
}

```

---

### 2. `WebAgent` 구조체 및 구현부 (히스토리 적용)

```rust
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use async_trait::async_trait;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::Ollama;

pub struct WebAgent<'a, P: PageTrait> {
    ollama: Ollama,
    model: String,
    page: &'a P,
    task_goal: String,
    target_description: String,
    // 💡 스텝별 행동 기록을 누적 보관하는 필드 추가
    history: Vec<ActionHistory>,
    max_history_depth: usize, // 토큰 폭발 방지를 위한 최근 기록 유지 개수
}

impl<'a, P: PageTrait> WebAgent<'a, P> {
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
            max_history_depth: 5, // 최근 5개 스텝의 히스토리만 LLM에 전달
        }
    }

    pub async fn run(&mut self) -> DynResult<()> {
        let mut step = 1;
        loop {
            println!("\n=== Agent Step {} ===", step);

            // 1. Observe: 현 시점 DOM 캡처
            let obs = AgentObservation::capture(self.page).await?;

            // 2. Think: 누적된 히스토리를 포함하여 LLM 추론 요청
            let mut action_result = None;
            for attempt in 1..=3 {
                match self.ask_step(step, &obs.simplified_dom).await {
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

            // 3. Record: 이번 스텝의 사고 및 행동을 히스토리에 기록
            let history_entry = ActionHistory {
                step,
                reasoning: action.reasoning().to_string(),
                action_summary: format!("{:?}", action),
            };
            self.history.push(history_entry);

            // 4. Act: 행동 수행
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

    /// 히스토리를 텍스트 블록 형태로 가공해 주는 헬퍼 메서드
    fn build_history_context(&self) -> String {
        if self.history.is_empty() {
            return "None (This is the initial step)".to_string();
        }

        // 최근 max_history_depth 개수만큼의 히스토리만 슬라이싱
        let start_idx = self.history.len().saturating_sub(self.max_history_depth);
        let recent_history = &self.history[start_idx..];

        let mut context = String::new();
        for item in recent_history {
            context.push_str(&format!(
                "Step {}: Reasoning: {} => Action: {}\n",
                item.step, item.reasoning, item.action_summary
            ));
        }
        context
    }

    async fn ask_step(&self, current_step: usize, current_dom: &str) -> DynResult<AgentAction> {
        let system_prompt = r###"
You are an automated web agent and data extraction assistant.
Before choosing an action, you MUST explain your rationale in detail within the 'reasoning' field.
Review the 'Previous Action History' to avoid repeating unnecessary actions or getting stuck in loops.
Respond ONLY with a valid JSON object matching one of the schemas below. Do NOT include any extra text or Markdown formatting.

1. Write File: {"action": "WriteFile", "params": {"reasoning": "...", "filename": "output.json", "content": "..."}}
2. Scroll Page: {"action": "ScrollPage", "params": {"reasoning": "...", "dy": 1500}}
3. Click Element: {"action": "ClickElement", "params": {"reasoning": "...", "selector": "#btn"}}
4. Finish Task: {"action": "Finish", "params": {"reasoning": "...", "reason": "Task completed"}}
5. Extract Data: {"action": "ExtractData", "params": {"reasoning": "...", "selectors": ["..."]}}
"###;

        // 히스토리 맥락 추출
        let history_str = self.build_history_context();

        let prompt = format!(
            "Target Goal: {}\n\nPrevious Action History:\n{}\n\nCurrent Step: {}\nCurrent DOM State:\n{}\n\nDetermine the next action:",
            self.task_goal, history_str, current_step, current_dom
        );

        let request = GenerationRequest::new(self.model.clone(), prompt)
            .system(system_prompt.to_string())
            .format(FormatType::Json);

        let llm_response = self.ollama.generate(request).await?;
        let response_text = llm_response.response.trim();
        let clean_json = lib::page::parse_json_from_llm_response(response_text);

        let action: AgentAction = serde_json::from_str(&clean_json)?;

        println!("\n🧠 [Agent Thinking]: {}", action.reasoning());
        println!("🎯 [Agent Action]: {:?}", action);

        Ok(action)
    }

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
                let selectors = lib::page::extract_selectors_via_local_llm(
                    &obs.simplified_dom,
                    &self.target_description,
                    None,
                    Some(self.model.clone()),
                    &self.ollama,
                )
                .await?;
                let extracted_data =
                    lib::page::extract_data_with_selectors(self.page, selectors).await?;
                println!("추출된 데이터: {:?}", extracted_data);
            }
        }
        Ok(LoopControl::Continue)
    }
}

```

---

### 3. `main` 함수 사용 시 변화 (`mut` 적용)

`run()` 내부에서 `self.history.push(...)`를 통해 가변 상태를 변경해야 하므로 `agent` 변수를 `mut`로 선언해 주시면 됩니다.

```rust
#[tokio::main]
async fn main() -> DynResult<()> {
    // ... 브라우저 및 페이지 생성 ...
    let page_adapter = ChromiumPageAdapter::new(raw_page);

    // mut 키워드 사용
    let mut agent = WebAgent::new(
        "http://localhost",
        31434,
        "qwen3:14b",
        &page_adapter,
        "collect 50 items and details",
        "From the list of items, extract: content, username, href, img_src",
    );

    // 실행
    agent.run().await?;

    Ok(())
}

```

이렇게 하면 LLM 프롬프트상에 아래처럼 이전 스텝 기록이 계속 포함되어 전달되므로, 동일한 버튼 클릭이나 의미 없는 무한 스크롤을 방지하는 정밀도가 크게 향상됩니다.

```text
Previous Action History:
Step 1: Reasoning: I need to scroll down to see more items. => Action: ScrollPage { reasoning: "...", dy: 1500 }
Step 2: Reasoning: Now click the 'Load More' button. => Action: ClickElement { reasoning: "...", selector: "#more_btn" }

```
