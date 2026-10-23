너무 복잡함. 새로운 페이지 접근 시 기존에 존재하는 selector를 먼저 적용해서 데이터를 추출해 보고 데이터 추출이 되면 그걸 그대로 쓰고 아니면 새로 selector를 추출하는 방식으로 바꿀래.

아주 실용적이고 확실한 접근 방식입니다! URL 라우팅을 복잡하게 추론할 필요 없이, **"기존 성공 셀렉터 재시도 (Quick Retry) → 실패 시 LLM 셀렉터 추론 (Fallback)"** 구조로 바꾸면 에이전트 동작 속도가 훨씬 빨라지고 토큰도 아낄 수 있습니다.

---

### 동작 흐름 (Strategy)는

1. **새 페이지 도달**
2. **캐시 확인:** 메모리/파일에 저장된 셀렉터 목록이 있다면 **먼저 현재 DOM에 적용**해 봅니다.
3. **결과 검증:**

- **성공 (유효한 데이터 추출됨):** LLM 추론 스텝을 완전히 건너뛰고 바로 데이터를 저장한 뒤 다음 액션을 진행합니다.
- **실패 (요소를 못 찾거나 데이터가 비어있음):** 그때 LLM을 호출하여 새로운 셀렉터를 추론 및 추출(`ExtractData`)하도록 패스합니다.

---

### Rust 구현 코드

이 로직을 `handle_action`이나 별도의 관측/실행 단계에 적용한 구조입니다.

```rust
impl WebAgent {
    /// 페이지 접근 시 먼저 기존 셀렉터로 즉시 추출을 시도하는 메서드
    pub async fn try_extract_with_cached_selectors(&mut self) -> DynResult<Option<serde_json::Value>> {
        // 1. 메모리에 저장된 기존 셀렉터가 없다면 즉시 스킵
        if self.cached_selectors.is_empty() {
            return Ok(None);
        }

        println!("⚡ [Quick Check] 기존에 저장된 셀렉터로 데이터 추출을 시도합니다...");

        // 2. 기존 셀렉터로 DOM에서 추출 시도
        match crate::lib::page::extract_data_with_selectors(self.page, &self.cached_selectors).await {
            Ok(data) if !Self::is_data_empty(&data) => {
                println!("✅ [Quick Check 성공] 기존 셀렉터가 유효합니다. LLM 추론을 건너뜁니다.");
                Ok(Some(data))
            }
            _ => {
                println!("⚠️ [Quick Check 실패] 기존 셀렉터로 데이터를 찾지 못했습니다. LLM 추론으로 전환합니다.");
                Ok(None)
            }
        }
    }

    /// 추출된 데이터가 비어있는지 검증하는 헬퍼
    fn is_data_empty(data: &serde_json::Value) -> bool {
        match data {
            serde_json::Value::Null => true,
            serde_json::Value::Array(arr) => arr.is_empty(),
            serde_json::Value::Object(obj) => obj.is_empty(),
            _ => false,
        }
    }
}

```

---

### 메인 루프(`run`) 적용 방법

`Agent Step` 루프 시작 부분에서 관측(Observe) 직후 시도하도록 배치하면, **LLM 요청(`ask_step`) 자체를 안 거치므로 실행 속도가 획기적으로 향상**됩니다.

```rust
// 메인 루프 내부
loop {
    println!("\n=== Agent Step {} ===", step);

    // 1. Observe: 스텝 시작 시 관측
    let obs = AgentObservation::capture(self.page).await?;

    // 2. Fast Path: 기존 셀렉터 재사용 시도
    if let Ok(Some(extracted_data)) = self.try_extract_with_cached_selectors().await {
        // 기존 셀렉터로 바로 추출 성공했으므로 파일에 Append 저장
        self.append_item_to_file("items_detail.json", extracted_data).await?;

        // 히스토리에 기록
        self.history.push(ActionHistory {
            step,
            action_name: "QuickExtractData".to_string(),
            reasoning: "Reused existing cached selectors successfully.".to_string(),
            status: "Success (Cached)".to_string(),
        });

        step += 1;
        continue; // 👈 LLM 호출(Think) 생략하고 다음 루프로!
    }

    // 3. Slow Path (LLM Think): 기존 셀렉터 실패시에만 LLM에 물어봄
    let action = self.ask_step_with_retry(&obs.simplified_dom).await?;

    // ... 이후 handle_action 및 기록 동일 ...
}

```

### 이 방식의 장점

1. **속도 극대화:** 상세 페이지들을 연속으로 순회할 때 2번째 아이템부터는 **LLM 호출 없이 0.1초만에 데이터 추출** 후 다음 페이지로 넘어갑니다.
2. **비용 및 토큰 절감:** 동일한 구조의 페이지에서는 LLM API 요청을 거의 아예 안 쓰게 됩니다.
3. **자가 회복력:** 페이지 구조가 살짝 바뀌어 기존 셀렉터가 깨지면, 자동으로 `Fast Path`가 실패하면서 LLM이 새로 셀렉터를 찾도록 낙하(Fallback)합니다.
