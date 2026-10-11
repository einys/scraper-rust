ARCHITECTURE.md

Okay, let's break down the steps to implement the Rust-based scraper you outlined in the documentation. I'll focus on translating that architectural overview into a practical plan.

**1. Project Setup and Dependencies**

First, let's create a new Rust project.

```bash
cargo new rust_scraper
cd rust_scraper
```

Next, we'll add the necessary dependencies to your `Cargo.toml` file. Based on the documentation, you'll need:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
ollama-rs = "0.3"
tokio-stream = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chromiumoxide = "0.7" # Or potentially 'thirtyfour' for Selenium compatibility
```

**2. Core Modules and Functions**

Based on the architecture, we'll need modules/functions for:

- `rrweb` data parsing and feature extraction
- DOM/Route graph mapping
- Local LLM inference/agent logic
- Mouse/Keyboard event synthesis
- CDP control (Chromiumoxide or Thirtyfour)

Here's a basic outline for the structure. I'll focus on the core Chromiumoxide/Ollama integration first, and then touch on the other areas.

```rust
// src/main.rs

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::handler::viewport::Viewport;
use futures::StreamExt;
use serde_json::json;
use std::time::Duration;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use tokio::io::{self, AsyncWriteExt};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Launch Headless Chrome
    let (browser, mut handler) = Browser::launch(
        BrowserConfig::builder()
            .viewport(Viewport { width: 1920, height: 1080, ..Default::default() })
            .build()?
    ).await?;

    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() { break; }
        }
    });

    let page = browser.new_page("https://target-website.com").await?;
    page.wait_for_navigation().await?;

    // Simulate scrolling
    for _ in 0..5 {
        page.evaluate("window.scrollBy(0, 1500);").await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    //Get final dom html
    let html_content = page.content().await?;

    // Connect to K8s Ollama NodePort (31434)
    let ollama = Ollama::new("http://localhost".to_string(), 31434);
    let model = "qwen2.5:14b".to_string();
    let prompt = format!(
        "다음 HTML에서 상품명(title)과 가격(price)만 추출하여 순수 JSON 배열 포맷으로 반환해 줘:\n\n{}",
        html_content.chars().take(12000).collect::<String>()
    );

    let request = GenerationRequest::new(model, prompt);

    println!("K8s Ollama 모델(qwen2.5:14b) 추론 요청 중...");

    match ollama.generate(request).await {
        Ok(res) => {
            println!("\n--- 추출 결과 ---");
            println!("{}", res.response);
        }
        Err(e) => {
            eprintln!("Ollama 호출 실패: {:?}", e);
        }
    }

    Ok(())
}
```

**3. Chromiumoxide or Thirtyfour Integration**

- **Chromiumoxide:** For the fastest performance and fine-grained control, I've used Chromiumoxide in the example. You'll need to implement the scrolling and other browser actions based on the target website's behavior.
- **Thirtyfour:** If you prefer a more familiar Selenium-like API, you can substitute `thirtyfour` for `chromiumoxide`. You'll need to adjust the code accordingly.

**4. Ollama Integration**

The code includes the `ollama-rs` integration as shown in the documentation. You'll need to ensure your K8s Ollama server is running and accessible at the specified port.

**5. Error Handling**

The provided code includes basic error handling using `Result<(), Box<dyn std::error::Error>>`. You should expand on this to handle specific errors more gracefully.

**Next Steps & Considerations**

- **Target Website Analysis:** You need to carefully analyze the target website's DOM structure and JavaScript behavior to implement accurate scrolling, mouse movements, and data extraction.
- **`rrweb` Data Processing:** The architecture mentions `rrweb` data processing. You'll need to integrate the `rrweb` data and build the feature extractor module.
- **DOM/Route Graph Mapping:** Implement a module to map DOM structure and routes to the extracted `rrweb` data.
- **LLM Logic:** Develop the local LLM inference/agent logic to make decisions based on the extracted data.
- **Testing and Refinement:** Thoroughly test your scraper and refine it as needed. Consider adding unit tests and integration tests.

I'll start with the basic scaffolding and the `chromiumoxide`/`ollama-rs` integration.
