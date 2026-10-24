Ollama Native Tool Calling API를 쓰지 않더라도, 지금처럼 `AgentAction` enum과 `serde_json` 조합으로 Structured Output(JSON Schema)을 받는 구조는 다음과 같은 강력한 이점이 있습니다.

1. **로컬 LLM(Qwen 등)과의 최적의 호환성:** 모델에 따라 Native Tool Calling 지원 여부나 JSON 포맷팅 품질이 다른데, 시스템 프롬프트로 Enum 파싱 형태를 강제하면 에러율이 낮아집니다.
2. **Rust의 강력한 타입 안정성 (Type Safety):** LLM이 잘못된 인자를 넘기거나 오타를 내면 `serde_json::from_str` 단계에서 즉시 에러가 잡혀, 실행부(`handle_action`)에서 예외가 발생하지 않습니다.
3. **독립적인 Tool 확장성:** 새 기능(예: `BackPage`, `HoverElement`)을 추가하고 싶다면 `AgentAction` enum에 한 줄 추가하고 `match` 분기만 늘려주면 됩니다.
