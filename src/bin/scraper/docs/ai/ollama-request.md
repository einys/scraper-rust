# request 보내는 법 template

```rust
    // Connect to K8s Ollama NodePort (31434)
    let ollama = Ollama::builder()
        .host("http://localhost".to_string())
        .port(31434)
        .build();
    let model = "qwen3:14b".to_string();

    let prompt = format!(
        "현재 페이지 DOM:\n{}\n\n다음으로 수행할 행동은?",
        current_dom
    );

    // Ollama 설정
    let request = GenerationRequest::new(model.to_string(), prompt)
        .system(system_prompt.to_string())
        .format(FormatType::Json);

    let llm_response = ollama.generate(request).await?;
```
