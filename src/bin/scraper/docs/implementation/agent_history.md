- dynamic prompt
- "Context Push / Pop Pattern" 또는 "Ephemeral Context

Readfile / ExtractData 이 두 action의 결과 data는 필요할 때 잠시 쓰고 pop 시켜야 함
---

how can I imporve agent to use memory and prompt efficiently? For example agent can save past data into a file and load it when it think it needs it. Agent can decide which file they need by action history, which has the filename saved or loaded. but I still do not know how this agent can actually load the content to act next step.

qwen2.5:14b
To enhance the agent's ability to use memory efficiently and load past data when needed, we can implement a mechanism where the agent saves and loads data from a file based on its action history. This can be achieved by modifying the `WebAgent` struct and its associated methods to manage a memory store and provide functions to save and load data.

Here's a step-by-step approach to implement this:

1. **Modify the `WebAgent` struct** to include a memory store and methods for saving and loading data.
2. **Add methods** to save and load data based on the action history.
3. **Update the `run` method** to use the memory store when necessary.

Let's go through these steps one by one:

### Step 1: Modify the `WebAgent` struct

We'll add a `memory` field to the `WebAgent` struct to store the data that the agent needs to remember between actions.

```rust
use std::collections::HashMap;

pub struct WebAgent<'a, P> {
    // Existing fields...
    pub memory: HashMap<String, String>,
    // ...
}

impl<'a, P> WebAgent<'a, P>
where P: AsRef<Path> {
    pub async fn new(...) -> Self {
        // Existing code...
        let memory = HashMap::new();
        WebAgent { memory, .. }
    }
}
```

### Step 2: Add methods to save and load data

We'll add methods to save data to a file and load data from a file based on the action history.

#### Save Data Method

```rust
async fn save_data(&self, filename: &str, data: &str) -> Result<()> {
    write_file(format!("data/{}", filename), data).await?;
    Ok(())
}
```

#### Load Data Method

```rust
async fn load_data(&self, filename: &str) -> Result<String> {
    read_file(format!("data/{}", filename)).await
}
```

### Step 3: Update the `run` method

We'll modify the `run` method to use the memory store when necessary and update the memory store with new data.

#### Example Update in `handle_action`

```rust
async fn handle_action(&mut self, action: &AgentAction) -> LoopControl {
    match action {
        AgentAction::ClickElement { reasoning, selector } => {
            let element = self.page.wait_for_selector(&selector).await.unwrap();
            element.click().await.unwrap();
            let history_item = ActionHistory {
                step: self.action_counter,
                reasoning: reasoning.clone(),
                action_name: "ClickElement".to_string(),
                status: "Successfully clicked element".to_string(),
                data: None,
            };
            self.history.push(history_item);
            self.action_counter += 1;

            // Save data to file after clicking
            let file_name = format!("{}.json", selector); // Example filename based on selector
            self.save_data(&file_name, "Some data to save").await.unwrap();

            LoopControl::Continue
        },
        // Similar modifications for other actions
        // ...
    }
}
```

#### Example Use of Memory in `handle_action`

```rust
async fn handle_action(&mut self, action: &AgentAction) -> LoopControl {
    match action {
        AgentAction::ExtractData { reasoning } => {
            // Check if we need to load data from a file
            let filename = self.history.last().unwrap().data.as_ref().unwrap();
            let data = self.load_data(filename).await?;

            // Use the data to perform the action
            let processed_data = process_data(&data); // Example function to process data
            self.memory.insert("processed_data".to_string(), processed_data);

            LoopControl::Continue
        },
        // Similar modifications for other actions
        // ...
    }
}
```

### Putting It All Together

Here's the complete updated `WebAgent` struct and methods:

```rust
use std::collections::HashMap;
use std::path::Path;

use crate::agent::AgentAction;
use crate::action_history::ActionHistory;

pub struct WebAgent<'a, P> {
    pub page: Page,
    pub task_goal: usize,
    pub history: Vec<ActionHistory>,
    pub action_counter: usize,
    pub memory: HashMap<String, String>,
    // ...
}

impl<'a, P> WebAgent<'a, P>
where P: AsRef<Path> {
    pub async fn new(...) -> Self {
        let memory = HashMap::new();
        WebAgent { memory, .. }
    }

    async fn save_data(&self, filename: &str, data: &str) -> Result<()> {
        write_file(format!("data/{}", filename), data).await?;
        Ok(())
    }

    async fn load_data(&self, filename: &str) -> Result<String> {
        read_file(format!("data/{}", filename)).await
    }

    async fn handle_action(&mut self, action: &AgentAction) -> LoopControl {
        match action {
            AgentAction::ClickElement { reasoning, selector } => {
                let element = self.page.wait_for_selector(&selector).await.unwrap();
                element.click().await.unwrap();
                let history_item = ActionHistory {
                    step: self.action_counter,
                    reasoning: reasoning.clone(),
                    action_name: "ClickElement".to_string(),
                    status: "Successfully clicked element".to_string(),
                    data: None,
                };
                self.history.push(history_item);
                self.action_counter += 1;

                let file_name = format!("{}.json", selector);
                self.save_data(&file_name, "Some data to save").await.unwrap();

                LoopControl::Continue
            },
            AgentAction::ExtractData { reasoning } => {
                let filename = self.history.last().unwrap().data.as_ref().unwrap();
                let data = self.load_data(filename).await?;
                let processed_data = process_data(&data);
                self.memory.insert("processed_data".to_string(), processed_data);

                LoopControl::Continue
            },
            // Other actions...
        }
    }
}
```

This setup allows the agent to save and load data efficiently, leveraging its action history to determine which files to load.

Would you like to proceed with applying these changes to the file `agent.rs` and running tests to validate the changes? If so, let's start by modifying the file as outlined above.

제시해주신 접근 방식의 아이디어(메모리 영속화 및 파일 기반 데이터 로딩)는 에이전트의 상태 보존 측면에서 아주 훌륭한 방향입니다. 하지만 **현재 제시된 예시 코드에는 Rust 컴파일 및 실제 에이전트 동작 시 심각한 오류를 발생시킬 몇 가지 치명적인 문제점**이 있습니다.

구체적인 문제점과 이를 제대로 보완하기 위한 개선 방안을 정리해 드립니다.

---

### 1. 현재 예시 코드의 치명적인 문제점

1. **`unwrap()`으로 인한 런타임 패닉 발생 위험**

- `self.history.last().unwrap().data.as_ref().unwrap()`
- 첫 번째 스텝이거나 이전 히스토리의 `data`가 `None`인 경우 **에이전트가 즉시 crash(패닉)** 됩니다.

2. **파일명으로 CSS 셀렉터 사용 불가능 (`ClickElement`)**

- `let file_name = format!("{}.json", selector);`
- `selector`에는 `.class`, `#id`, `div > a[href*='...']` 같은 특수문자(`:`, `>`, `[`, `]`, `/`)가 대량 포함되어 있어서 OS 파일 시스템 경로 오류(Invalid Filename)로 파일 작성이 실패합니다.

3. **`memory: HashMap<String, String>`의 타입 한계**

- 단순 `String` 매핑 방식으로는 `ExtractionResult`나 JSON 배열 같은 구조화된 데이터를 효율적으로 제어하거나 중복 검사(Diffing)를 수행하기 어렵습니다.

4. **역할(Role)의 역전**

- `ClickElement` 시점에 무조건 `save_data`를 호출하는 것은 원치 않는 가짜 데이터 저장을 남기며, `ExtractData`에서 DOM 추출 대신 파일 로드를 시도하는 로직은 Web Scraper의 본래 목적과 맞지 않습니다.

---

### 2. 올바른 설계 방향 (추천 모델)

메모리와 파일 Persistence를 연동하려면 다음과 같이 구조화하는 것이 훨씬 안전하고 견고합니다.

1. **`last_seen_page_data` 구조화**: 단순 `HashMap<String, String>` 대신 `Option<ExtractionResult>` 또는 타입 정의된 구조체 사용.
2. **파일 저장 시 안전한 경로명 쓰기**: `selector` 대신 `route_key`("items_list", "items_detail") 기반의 파일명 작성.
3. **`Option` 안전 처리 (`if let` / `match`)**: `unwrap()` 대신 안전하게 패턴 매칭하여 파일이 없거나 메모리가 비어있을 때 패닉 방지.

---

### 3. 올바르게 수정된 Rust 구현 예시

```rust
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

pub struct WebAgent<'a, P: PageTrait> {
    pub page: &'a P,
    pub task_goal: String,
    pub history: Vec<ActionHistory>,
    pub action_counter: usize,

    // 💡 1. 단순 String이 아닌 구조화된 ExtractionResult 또는 JSON Value 메모리 저장
    pub last_seen_page_data: Option<ExtractionResult>,
    pub selectors_cache: HashMap<String, SelectorSchema>,
}

impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// 안전한 파일 데이터 저장 메서드
    async fn save_data(&self, filename: &str, data: &serde_json::Value) -> DynResult<()> {
        let dir = Path::new("data");
        if !dir.exists() {
            fs::create_dir_all(dir).await?;
        }

        let file_path = dir.join(filename);
        let json_str = serde_json::to_string_pretty(data)?;
        fs::write(file_path, json_str).await?;
        Ok(())
    }

    /// 안전한 파일 데이터 로드 메서드
    async fn load_data(&self, filename: &str) -> DynResult<Option<serde_json::Value>> {
        let file_path = Path::new("data").join(filename);
        if !file_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(file_path).await?;
        let json_val: serde_json::Value = serde_json::from_str(&content)?;
        Ok(Some(json_val))
    }

    /// 올바른 handle_action 내 데이터 연동
    pub async fn handle_action(
        &mut self,
        action: &AgentAction,
        obs: &AgentObservation,
    ) -> DynResult<(LoopControl, String)> {
        match action {
            AgentAction::ExtractData { reasoning, schema } => {
                // 1. DOM 추출 실행
                let raw_json = crate::lib::page::extract_data_with_selectors(self.page, schema).await?;
                let extraction_result: ExtractionResult = serde_json::from_str(&raw_json)?;

                // 2. 에이전트 내부 메모리 갱신
                self.last_seen_page_data = Some(extraction_result.clone());

                // 3. 라우트 키 기반으로 안전하게 파일 작성 (Selector 특수문자 피함)
                let route_key = self.determine_route_key(&obs.current_url);
                let filename = format!("{}.json", route_key);

                let json_value = serde_json::to_value(&extraction_result)?;
                self.save_data(&filename, &json_value).await?;

                Ok((
                    LoopControl::Continue,
                    format!("Successfully extracted and saved data to 'data/{}'", filename),
                ))
            }

            AgentAction::ReadFile { filename, reasoning } => {
                // unwrap 대신 안전한 load_data 호출
                match self.load_data(filename).await {
                    Ok(Some(data)) => Ok((
                        LoopControl::Continue,
                        format!("Loaded data from 'data/{}': {}", filename, data),
                    )),
                    Ok(None) => Ok((
                        LoopControl::Continue,
                        format!("File 'data/{}' does not exist.", filename),
                    )),
                    Err(e) => Ok((
                        LoopControl::Continue,
                        format!("Failed to load 'data/{}': {}", filename, e),
                    )),
                }
            }

            _ => { /* 기타 액션 처리 */ Ok((LoopControl::Continue, "Executed".to_string())) }
        }
    }
}

```

---

### 결론

**개념과 의도는 아주 좋으나, 제안해주신 코드를 그대로 적용하시면 `unwrap()` 패닉 및 특수문자 파일명 생성 오류로 에이전트가 바로 멈추게 됩니다.**

따라서 위와 같이 **1) unwrap 제거**, **2) 라우트 키 기반 정제된 파일명 사용**, **3) `ExtractionResult` 타입 저장** 방식으로 정돈하여 적용하는 것을 강력히 권장합니다.

# history랑 data가 다 같이 들어있으면 너무 길어지는데 history를 원할 때 그것만 따로, data를 같이 원할 때 같이 프롬프트로 받게 분기를 나누는게 좋을까?

맞습니다. **`History`와 `Data`를 매 스텝마다 통째로 넘기면 토큰 소모가 극심해지고, 모델의 집중력이 흐려져 잘못된 JSON을 출력할 확률이 급격히 높아집니다.**

따라서 필요한 순간에만 선택적으로 정보를 주입하는 **컨텍스트 분기(Dynamic Prompt Context) 구조**로 만드는 것이 훨씬 효율적이고 스마트합니다.

---

### 프롬프트 컨텍스트 분기 설계 방안

LLM에 전달하는 프롬프트 입력을 상황에 따라 **3가지 가벼운 모드**로 분기하는 구조를 추천합니다.

#### 1. 기본 추론 모드 (Lightweight Step - 90%의 상황)

평소 다음 행동을 결정할 때는 무거운 전체 Data 대신 **경량화된 History 요약만** 포함시킵니다.

- **포함 정보:**
- Task Goal (목표)
- Current Observation (현재 DOM)
- Recent History (최근 3~5개 Action의 이름과 1줄 Status 요약)

- **결과:** 토큰 소모 최소화, 빠른 추론 속도 유지.

#### 2. 데이터 판별/비교 모드 (Data Inspection)

`ExtractData` 직후나, 에이전트가 "이 항목을 새로 저장해야 하나?" 판단이 필요한 시점에만 `Data` 요약을 포함시킵니다.

- **포함 정보:**
- Basic Step 정보
- `last_seen_page_data`의 핵심 요약 (예: 현재 아이템 ID, 타이틀 등)
- 기존 파일에 저장된 주요 Key 목록 (전체 JSON이 아닌 **ID/URL 목록 문자열만**)

- **결과:** LLM이 중복 여부를 명확히 파악하고 `WriteFile` 또는 `ClickElement`로 자연스럽게 분기.

#### 3. 파일/메모리 조회 요청 시 (Explicit Read)

LLM이 스스로 `ReadFile` 액션을 수행했거나, 명시적으로 이전 수집 기록 확인을 원할 때만 파일 원문을 프롬프트 컨텍스트에 주입합니다.

---

### Rust 코드 구현 예시 (프롬프트 동적 생성)

`ask_step` 호출 전, 현재 상태에 따라 프롬프트 메시지를 조립하는 헬퍼 메서드 패턴입니다.

```rust
impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// 현재 상황에 맞게 최적화된 시스템/유저 프롬프트를 동적으로 생성
    pub fn build_prompt_context(&self, simplified_dom: &str) -> String {
        let history_summary = self.get_compact_history(5); // 최근 5개 액션 요약

        let mut context = format!(
            "Goal: {}\n\nHistory:\n{}\n\nCurrent DOM:\n{}",
            self.task_goal, history_summary, simplified_dom
        );

        // 💡 직전 액션이 ExtractData였고, 데이터 비교가 필요한 순간에만 Data 컨텍스트 추가 주입
        if let Some(last_history) = self.history.last() {
            if last_history.action_name == "ExtractData" {
                if let Some(data) = &self.last_seen_page_data {
                    let data_preview = self.format_data_summary(data);
                    context.push_str(&format!("\n\n[Extracted Data Preview]:\n{}", data_preview));
                }
            }
        }

        context
    }

    /// History를 최근 N개만 1줄로 요약하여 토큰 절약
    fn get_compact_history(&self, limit: usize) -> String {
        self.history
            .iter()
            .rev()
            .take(limit)
            .rev()
            .map(|h| format!("Step {}: Action='{}' | Status='{}'", h.step, h.action_name, h.status))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

```

---

### 핵심 요약

1. **상시 전달 금지:** `Data` 전체를 매 스텝 프롬프트에 넣으면 토큰 폭발과 둔화 현상이 생깁니다.
2. **동적 프롬프트 구성:** 평소에는 **최근 N개 History(1줄 요약)만**, `ExtractData` 직후나 판단이 필요한 특정 스텝에만 `Data` 요약(Preview)을 동적으로 덧붙이는 분기 방식이 가장 좋습니다.

# push pop을 쓰는 구조는 어때? 필요할 때 readfile 해서 push하고 writefile한 후에 pop시키고?

`push` / `pop` 형태의 스택(Stack) 또는 윈도우(Window) 방식으로 컨텍스트 메모리를 제어하는 아이디어는 **토큰 관리와 컨텍스트 가독성 측면에서 매우 뛰어난 구조**입니다!

실제로 에이전트 개발에서 이를 **"Context Push / Pop Pattern"** 또는 "Ephemeral Context"라고 부르며, 복잡한 데이터가 필요한 특정 스텝에서만 메모리를 밀어 넣었다가 작업이 끝나면 제거하여 프롬프트를 청정하게 유지하는 기법입니다.

---

### Push / Pop 제어 흐름 구조

```
[기존 메인 프롬프트] -> (Task Goal + Recent History + Current DOM)
       │
       ├─ (1) ReadFile 또는 ExtractData 실행
       │      └─ PUSH: 읽어온 Data/결과물 요약을 temp_context에 추가
       │
       ├─ (2) LLM 추론 스텝 (데이터를 보고 판단 및 WriteFile/Click 수행)
       │
       └─ (3) 작업 완료
              └─ POP: temp_context에서 해당 Data 제거 (기존 메인 프롬프트로 복귀)

```

---

### 추천하는 Rust 데이터 구조 (`temp_context`)

`WebAgent` 구조체에 스택 역할(또는 `Option`/`Vec`)을 하는 임시 컨텍스트 필드를 두고, `ask_step` 시점에만 이를 메인 프롬프트 뒤에 결합해 줍니다.

```rust
pub struct WebAgent<'a, P: PageTrait> {
    // 기존 필드들...
    pub history: Vec<ActionHistory>,

    // 💡 프롬프트에 세션 단위로 일시 주입할 스택 데이터 (Push / Pop)
    pub temp_context_stack: Vec<String>,
}

impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// 임시 컨텍스트 추가 (PUSH)
    pub fn push_context(&mut self, context_data: String) {
        println!("📥 [Memory Push] 프롬프트 컨텍스트에 데이터 주입");
        self.temp_context_stack.push(context_data);
    }

    /// 사용이 끝난 컨텍스트 제거 (POP)
    pub fn pop_context(&mut self) -> Option<String> {
        let popped = self.temp_context_stack.pop();
        if popped.is_some() {
            println!("📤 [Memory Pop] 프롬프트 컨텍스트에서 데이터 제거");
        }
        popped
    }

    /// 프롬프트 생성 시 temp_context_stack의 내용을 자동으로 결합
    pub fn build_prompt(&self, simplified_dom: &str) -> String {
        let mut prompt = format!(
            "Goal: {}\n\nRecent History:\n{}\n\nCurrent DOM:\n{}",
            self.task_goal,
            self.get_compact_history(5), // 최근 5개 액션 요약
            simplified_dom
        );

        // 스택에 스냅샷 데이터가 쌓여있다면 일시적으로 뒤에 이어 붙임
        if !self.temp_context_stack.is_empty() {
            prompt.push_str("\n\n=== [Temporary Inspection Data] ===");
            for ctx in &self.temp_context_stack {
                prompt.push_str(&format!("\n{}", ctx));
            }
        }

        prompt
    }
}

```

---

### `handle_action` 연동 예시 (자동 Push / Pop)

```rust
// 1. ReadFile 실행 시 -> PUSH
AgentAction::ReadFile { filename, .. } => {
    let content = tokio::fs::read_to_string(&filename).await.unwrap_or_default();

    // 파일 내용 중 핵심 요약본을 스택에 Push
    let preview = format!("File '{}' Contents: {}", filename, content);
    self.push_context(preview);

    Ok((LoopControl::Continue, format!("Read file '{}' and pushed to context stack", filename)))
}

// 2. WriteFile 실행 완료 시 -> POP (사용한 데이터 소멸)
AgentAction::WriteFile { filename, content, .. } => {
    tokio::fs::write(&filename, content).await?;

    // 데이터 저장이 끝났으므로 컨텍스트 스택에서 POP 처리하여 프롬프트 비우기
    self.pop_context();

    Ok((LoopControl::Continue, format!("Wrote file '{}' and popped context stack", filename)))
}

```

---

### 이 패턴의 강력한 장점

1. **토큰 낭비 0%**: 데이터를 확인하고 기록하는 1~2 스텝 동안만 스택에 유지되므로, 전체 작업 루프의 평균 토큰 비용이 획기적으로 줄어듭니다.
2. **LLM 환각 및 혼란 방지**: 수십 스텝 전 읽었던 과거 데이터가 프롬프트에 계속 남아있어서 생기는 환각(Hallucination) 현상이 완전히 차단됩니다.
3. **명확한 Lifecycle**: `Read/Extract` $\rightarrow$ `Push` $\rightarrow$ `Write` $\rightarrow$ `Pop` 흐름이 명확하여 Rust의 라이프타임 관리처럼 깔끔합니다.
