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
