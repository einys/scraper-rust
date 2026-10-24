`AgentObservation`을 `WebAgent`의 내부 필드로 관리하고, 관측과 캐싱/추출 로직을 `WebAgent`의 `self` 메서드로 통합하는 것이 소유권 및 상태 관리 관점에서 훨씬 깔끔한 우수 설계(Best Practice)입니다.

---

### 구조적 이유

1. **`AgentObservation`은 상태 저장을 위한 DTO(Data Transfer Object)**
   `capture` 메서드가 LLM 호출(`self.extract_selectors`), 캐시(`self.selectors_cache`), 히스토리 관리 등 `WebAgent`의 내부 상태에 접근해야 하므로, 독립적인 static 메서드(`AgentObservation::capture(page)`)로 두는 순간 `page`뿐만 아니라 `agent` 상태까지 매개변수로 매번 전달해야 하는 파편화가 발생합니다.
2. **`self` 메서드로 전환 시 얻는 유연성**
   `pub async fn capture_observation(&mut self) -> DynResult<AgentObservation>` 형태로 `WebAgent`에 속하게 만들면, `self.page`, `self.selectors_cache`, `self.last_seen_page_data` 등에 자유롭게 접근할 수 있어 직관적입니다.

---

### 리팩토링 코드 예시

#### 1. `AgentObservation` 구조체 (순수 데이터 홀더)

```rust
#[derive(Debug, Clone)]
pub struct AgentObservation {
    pub simplified_dom: String,
    pub html_content: String,
    pub current_url: String,
    pub cached_selectors: Option<SelectorSchema>,
    pub last_extracted_data: Option<ExtractionResult>,
}

```

#### 2. `WebAgent` 내 메서드로 `capture` 통합

```rust
impl<'a, P: PageTrait> WebAgent<'a, P> {
    /// 현재 페이지 상태를 관측하고, 캐시된 셀렉터 및 이전 추출 데이터를 포함한 Observation 생성
    pub async fn capture_observation(&mut self) -> DynResult<AgentObservation> {
        let current_url = self.page.url().await.unwrap_or_default();
        println!("👀 Observing page {}...", current_url);

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

        // 2. Simplified DOM 및 Raw Content 수집
        let simplified_dom = crate::lib::page::get_simplified_dom_tree(self.page).await?;
        let html_content = self.page.content().await?;

        // 3. 캐시된 셀렉터 및 최근 추출 데이터 연동
        let cached_selectors = self.selectors_cache.get(&current_url).cloned();
        let last_extracted_data = self.last_seen_page_data.clone();

        Ok(AgentObservation {
            simplified_dom,
            html_content,
            current_url,
            cached_selectors,
            last_extracted_data,
        })
    }
}

```

#### 3. `run` 메인 루프에서의 호출 방식

```rust
pub async fn run(&mut self) -> DynResult<()> {
    loop {
        self.action_counter += 1;
        println!("\n=== Agent Step {} ===", self.action_counter);

        // self를 통과하여 관찰 관리를 단일 진입점으로 처리
        let obs = self.capture_observation().await?;

        // LLM 단계 추론 및 액션 수행
        let action = self.ask_step(&obs).await?;
        let (control, status_msg) = self.handle_action(&action, &obs).await?;

        if matches!(control, LoopControl::Finish) {
            break;
        }
    }
    Ok(())
}

```

---

### 장점 요약

- **상태의 단일 진입점(Single Source of Truth):** observation을 캡처할 때 셀렉터 캐시 조회, 인라인 전처리, 최근 추출 데이터 조회가 `WebAgent` 안에서 완벽히 일관되게 수행됩니다.
- **`P: PageTrait` 전달 최소화:** 매번 `AgentObservation::capture(self.page, &mut self.cache, ...)` 형태로 파라미터가 비대해지는 것을 막아줍니다.
