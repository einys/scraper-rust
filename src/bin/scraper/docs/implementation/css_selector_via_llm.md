리팩터링 포인트/과정 및 코드 전체 흐름 정리입니다.

---

### 🛠️ 코드 리팩터링 포인트 및 과정

1. **`AgentAction` 에이전트 추론 사고과정(`reasoning`) 필드 추가**

- 💡 **변경**: 모든 Enum 배리언트에 `reasoning: String` 추가 + `reasoning()` 헬퍼 메서드 구현
- 🎯 **효과**: LLM의 Chain-of-Thought(사고 과정)를 로깅/디버깅할 수 있게 됨

2. **어댑터 패턴(Adapter Pattern)을 통한 브라우저 추상화**

- 💡 **변경**: `ElementTrait` / `PageTrait` 정의 ➔ `ChromiumPageAdapter`, `ChromiumElementAdapter` 구현
- 🎯 **효과**: `chromiumoxide` 라이브러리 의존성을 격리하고 에이전트 로직(`handle_action`) 테스트/모킹 유연성 확보

3. **에러 타입 업캐스팅 오버헤드 방지**

- 💡 **변경**: `find_element`에서 `Err(e) => Err(Box::new(e) as DynError)` 처리
- 🎯 **효과**: Rust dyn 에러 변환 시 `Box<Box<...>>` 형태의 이중 박싱 오버헤드 방지

4. **비동기 I/O 최적화 및 헬퍼 정리**

- 💡 **변경**: `OpenOptions`를 활용한 비동기 파일 어펜드(`write_all`) + `tokio::fs::write` 도입
- 🎯 **효과**: 파일 입출력 시 컴파일 타임 타입 매칭 및 메모리 allocation 간소화

5. **`FormatType::Json` + Markdown 출력 정제**

- 💡 **변경**: Ollama 요청 시 `.format(FormatType::Json)` 지정 + ```json 코드블록 stripping 정제
- 🎯 **효과**: LLM 응답 파싱 실패율 최소화

---

### 🔄 전체 코드 실행 흐름

```text
[1. main() 시작]
  │
  ├─> Chromium 설정 (Headless, 1920x1080, Anti-Bot 플래그, User-Agent 지정)
  ├─> Browser::launch() ➔ 백그라운드 태스크로 핸들러 이벤트 루프 스폰 (Untagged Enum 에러 핸들링)
  │
[2. 페이지 이동 & 초기 스크롤]
  │
  ├─> target page ("[https://cmsn.info](https://cmsn.info)") 이동 및 페이지 로드 대기
  ├─> 3회 반복: window.scrollBy(0, 1500) ➔ 2초 대기 (동적 콘텐츠 로딩)
  │
[3. DOM 파싱 및 LLM 선택자 추출]
  │
  ├─> 원본 DOM(html_content) 및 경량화 DOM(simplified_dom) 추출
  ├─> extract_selectors_via_local_llm() 호출
  │     └─> Ollama(Qwen 등)에 전달 ➔ { "container": "...", "fields": { ... } } 스키마 획득
  │
[4. 파일 백업 & 결과 파싱]
  │
  ├─> page_data.html, page_simple.html, page_selectors.html 저장
  ├─> parse_items_with_schema() ➔ SelectorSchema 기반으로 원본 HTML에서 데이터 추출
  └─> serde_json::to_string_pretty() ➔ "page_result.json"에 최종 추출 결과 저장

```

---

### 🧠 (참고) `agent_loop` / `run_agent_step` 내부 흐름

```text
[Loop 시작]
  ├─ 1. Observe : DOM 획득 ➔ extract_clean_dom() ➔ 12,000자 Truncate
  ├─ 2. Think   : run_agent_step()
  │                 └─ System Prompt + FormatType::Json ➔ AgentAction 파싱
  │                 └─ 🧠 [Agent Thinking] 로깅
  └─ 3. Act     : handle_action()
                    ├─ WriteFile    ➔ OpenOptions (Append 모드 저장)
                    ├─ ScrollPage   ➔ window.scrollBy()
                    ├─ ClickElement ➔ find_element() ➔ click()
                    └─ Finish       ➔ LoopControl::Finish ➔ Loop 종료

```

## 싱글 아이템 페이지에서는 container가 "" (빈 문자열)

container가 있으면 배열, 없으면 싱글!

| 구분                 | 리스트 / 피드 페이지 (`/`)                                          | 싱글 아이템 페이지 (`/i/[id]`)                                  |
| -------------------- | ------------------------------------------------------------------- | --------------------------------------------------------------- |
| **데이터 구조**      | 1개 페이지에 **N개의 아이템 카드** 존재                             | 1개 페이지에 **1개의 아이템 상세** 존재                         |
| **`container` 역할** | 반복되는 각 카드들의 최상위 래퍼 선택자 (예: `div#masonry article`) | **불필요 (`""`)** 또는 페이지 본문 전체 (`main` / `body`)       |
| **추출 결과 타입**   | `ExtractionData::List(Vec<ExtractedItem>)`                          | `ExtractionData::Single(HashMap<String, ExtractedElementInfo>)` |

---

### 2. 싱글 아이템 페이지에서 `container`를 비워두는 이유

1. **상대 경로 대신 전역 CSS 선택자 사용 가능**

- 상세 페이지에서는 `username`, `content`, `updated_at` 등이 화면에 단 하나만 존재합니다.
- 굳이 부모 `container`를 거쳐 상대 경로로 접근할 필요 없이 `article header span.name`처럼 **문맥상 전역 고유한 CSS 선택자**를 바로 지정하는 것이 훨씬 간단하고 오진 확률이 낮습니다.

2. **단일 요소의 상세 스펙(좌표, OuterHTML) 수집 용이**

- 상세 페이지의 각 필드는 단순히 텍스트만 뽑는 것 외에, 클릭 위치(`rect`)나 HTML 구조(`outer_html`)를 함께 수집해야 할 때가 많습니다.
- 앞서 작성한 `get_element_info` 기반의 `ExtractionData::Single` 방식을 그대로 타는 것이 구조적으로 제일 깔끔합니다.

---

### 3. 하네스 처리 흐름

LLM이 생성한 `SelectorSchema`에서 `container` 필드의 존재 여부로 두 모드가 자동 전환되도록 구현해 두시면 됩니다.

```rust
// schema.container.trim().is_empty() 분기 조건
if schema.container.trim().is_empty() {
    // 📄 싱글 아이템 모드: 페이지 전역 단일 선택자로 추출
    // -> ExtractionData::Single(...)
} else {
    // 📑 피드/목록 모드: container 스코프 기반으로 여러 아이템 추출
    // -> ExtractionData::List(...)
}

```

> **Tip:** LLM 프롬프트에 **"Detail/Single pages usually do NOT need a container (leave `container` as `\"\"`), whereas Feed/List pages MUST specify a `container`."** 라는 지침을 명시해 두면 LLM이 페이지 종류에 따라 `container` 유무를 스스로 올바르게 결정합니다.

# container / field 구분을 치워버림 26-10-15

field에 배열이 반환되어서 deserialize오류가 남

해결: SelectorSchema를 배열과 문자열 모두 수용 가능하도록 유연화 (Rust 코드 보정)

LLM이 단일 셀렉터("a")를 보내든 배열(["a1", "a2"])을 보내든 **모두 하나의 셀렉터(쉼표 ,로 연결된 멀티 셀렉터)**로 자동 병합하도록 커스텀 디세리얼라이저를 적용하면 에러를 완벽히 차단할 수 있습니다.

아니야 배열로 보내면 따로따로 element를 가져와야 되는데?

CSS 구문상 쉼표(,)로 연결된 셀렉터문(예: a[href='...'], a[href='...'])을 document.querySelectorAll에 전달하면, 브라우저가 배열에 든 각 요소들을 독립된 개별 DOM Element로 각각 찾아냅니다.

즉, 배열 요소를 쉼표로 결합하여 단일 셀렉터 문자열로 다루어도 결과는 완전히 동일합니다.

- serde 의 Deserializer, deserialize_with 사용

```rust
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorSchema {
    #[serde(deserialize_with = "deserialize_fields")]
    pub fields: HashMap<String, String>,
}

fn deserialize_fields<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Single(String),
        List(Vec<String>),
    }

    let raw_map: HashMap<String, StringOrVec> = HashMap::deserialize(deserializer)?;
    let mut clean_map = HashMap::new();

    for (k, v) in raw_map {
        let joined_selector = match v {
            StringOrVec::Single(s) => s,
            // 배열로 들어오면 CSS 멀티 셀렉터(,)로 결합하여 각각의 요소를 다 수집함
            StringOrVec::List(list) => list.join(", "),
        };
        clean_map.insert(k, joined_selector);
    }

    Ok(clean_map)
}
```
