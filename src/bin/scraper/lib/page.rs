use chromiumoxide::Page;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::{generation::completion::request::GenerationRequest, Ollama};
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use tokio::test;

use crate::lib::dyn_result::DynError;
use crate::lib::dyn_result::DynResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Selector 단위 추출 정보 (위치 + 원본 내용)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedElementInfo {
    pub selector: String,
    pub text_content: Option<String>,
    pub outer_html: Option<String>,
    pub rect: Option<ElementRect>,
}

/// 최종 JSON 결과 (필드명 -> ExtractedElementInfo)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub data: HashMap<String, ExtractedElementInfo>,
}
use serde_json::{json, Value};

// selector 받고 -> 내용, 원본, x y 좌표 담긴 구조체 리턴
pub async fn get_element_info(
    page: &chromiumoxide::Page,
    selector: &str,
) -> Result<Option<ExtractedElementInfo>, DynError> {
    // JS 문자열 이스케이프 및 안전성을 위해 JSON.stringify 사용
    let selector_json = serde_json::to_string(selector)?;

    let js_script = format!(
        r#"
        (() => {{
            const el = document.querySelector({selector_json});
            if (!el) return null;
            const rect = el.getBoundingClientRect();
            return {{
                selector: {selector_json},
                text_content: el.innerText ? el.innerText.trim() : el.textContent.trim(),
                outer_html: el.outerHTML,
                rect: {{
                    x: rect.x + window.scrollX,
                    y: rect.y + window.scrollY,
                    width: rect.width,
                    height: rect.height
                }}
            }};
        }})()
        "#
    );

    // null인 경우 deserialization 에러를 피하고 Option<ExtractedElementInfo>로 매핑
    let result: Option<ExtractedElementInfo> = page.evaluate(js_script).await?.into_value()?;
    Ok(result)
}

/// 리턴: json형식의 '문자열'. LLM Selector 결과와 브라우저 Page를 받아 위치 및 내용이 통합된 JSON 결과를 반환합니다.
pub async fn extract_data_with_selectors(
    page: &chromiumoxide::Page,
    llm_selector_json_str: &str,
) -> Result<String, DynError> {
    // LLM 결과에서 JSON 부분 추출 (마크다운 ```json ... ``` 응답 대응)
    let clean_json_str = parse_json_from_llm_response(llm_selector_json_str);

    // Dynamic JSON 파싱 (필드명 -> Selector 문자열)
    let selectors_map: HashMap<String, Value> = serde_json::from_str(&clean_json_str)?;
    let mut final_data: HashMap<String, ExtractedElementInfo> = HashMap::new();

    for (field_name, selector_val) in selectors_map {
        if let Some(selector) = selector_val.as_str() {
            // 위치와 내용 원본을 한 번의 JS call로 취득
            if let Ok(Some(element_info)) = get_element_info(page, selector).await {
                final_data.insert(field_name, element_info);
            } else {
                // 요소를 찾지 못했거나 에러인 경우 None으로 채워 반환 구조 유지
                final_data.insert(
                    field_name,
                    ExtractedElementInfo {
                        selector: selector.to_string(),
                        text_content: None,
                        outer_html: None,
                        rect: None,
                    },
                );
            }
        }
    }

    let extraction_result = ExtractionResult { data: final_data };

    // 예쁘게 정렬된 JSON 문자열로 반환
    let json_output = serde_json::to_string_pretty(&extraction_result)?;
    Ok(json_output)
}

/// LLM 응답에 섞인 마크다운 코드블록 제거용 헬퍼 함수
fn parse_json_from_llm_response(raw_response: &str) -> String {
    let trimmed = raw_response.trim();
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return trimmed[start..=end].to_string();
        }
    }
    trimmed.to_string()
}

// ============================================================================
// 1. DOM 축약 간소화 트리 생성
// ============================================================================

/// 브라우저 DOM에서 크롤링/스크래핑 관점에서 의미 있는 핵심 태그, id, class, 주요 속성만 남긴 간소화된 DOM 텍스트 트리를 추출합니다.
pub async fn get_simplified_dom_tree(page: &Page) -> Result<String, DynError> {
    // DynError가 Box<dyn StdError + Send + Sync> 형태이므로,
    // 해당 에러 타입이 std::error::Error를 구현하고 있다면 ? 연산자에 의해
    // DynError로 자동 포장(From 변환)되어 상위 호출자로 즉시 반환됩니다.
    let js_script = r#"
    (() => {
        // 의미 있는 주요 태그 목록 (시각적/구조적 레이아웃 요소 위주)
        const SIGNIFICANT_TAGS = new Set([
            'body', 'header', 'nav', 'main', 'article', 'section', 'aside', 'footer',
            'div', 'ul', 'ol', 'li', 'table', 'thead', 'tbody', 'tr', 'th', 'td',
            'a', 'button', 'form', 'input', 'select', 'textarea', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'
        ]);

        function buildTree(node, depth = 0) {
            if (!node || node.nodeType !== Node.ELEMENT_NODE) return '';

            // 노드 태그명을 소문자로 변환하여 비교
            const tagName = node.tagName.toLowerCase();
            if (!SIGNIFICANT_TAGS.has(tagName)) return '';

            // hidden 속성이거나 display:none인 노드는 건너뜁니다.
            const style = window.getComputedStyle(node);
            if (style.display === 'none' || style.visibility === 'hidden') return '';

            const indent = '  '.repeat(depth);
            let attrStr = '';

            if (node.id) {
                attrStr += ` id="${node.id}"`;
            }
            if (node.className && typeof node.className === 'string' && node.className.trim() !== '') {
                // 클래스가 너무 길 경우 대표적인 클래스 위주로 정리
                const classes = node.className.trim().split(/\s+/).slice(0, 5).join(' ');
                attrStr += ` class="${classes}"`;
            }
            if (node.hasAttribute('href')) {
                attrStr += ` href="${node.getAttribute('href')}"`;
            }
            if (node.hasAttribute('name')) {
                attrStr += ` name="${node.getAttribute('name')}"`;
            }

            // 텍스트 노드가 직계 자식으로 포함되어 있다면 간략하게 요약 추가 (최대 30자)
            let directText = '';
            for (const child of node.childNodes) {
                if (child.nodeType === Node.TEXT_NODE) {
                    const text = child.textContent.trim();
                    if (text.length > 0) {
                        directText = text.length > 30 ? text.substring(0, 30) + '...' : text;
                        break;
                    }
                }
            }

            const textAttr = directText ? ` text="${directText}"` : '';
            let result = `${indent}<${tagName}${attrStr}${textAttr}>\n`;

            for (const child of node.children) {
                result += buildTree(child, depth + 1);
            }

            return result;
        }

        console.log('Starting buildTree with document.body');
        return buildTree(document.body || document.documentElement);
    })()
    "#;

    let tree: String = page.evaluate(js_script).await?.into_value()?;
    Ok(tree)
}

use std::{env, fmt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorSchema {
    pub container: String,
    pub fields: HashMap<String, String>,
}

impl fmt::Display for SelectorSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // JSON 문자열로 직렬화하여 Display에 전달
        match serde_json::to_string_pretty(self) {
            Ok(json_str) => write!(f, "{}", json_str),
            Err(_) => Err(fmt::Error),
        }
    }
}

// 3. SelectorSchema를 이용해 HTML 파싱 및 JSON Value로 반환하는 함수
pub fn parse_items_with_schema(raw_html: &str, schema: &SelectorSchema) -> Result<Value, DynError> {
    let document = Html::parse_document(raw_html);

    let container_selector = Selector::parse(&schema.container)
        .map_err(|e| format!("Invalid container selector '{}': {:?}", schema.container, e))?;

    let mut field_selectors: HashMap<String, Selector> = HashMap::new();
    for (field_name, selector_str) in &schema.fields {
        if let Ok(sel) = Selector::parse(selector_str) {
            field_selectors.insert(field_name.clone(), sel);
        } else {
            eprintln!(
                "⚠️ Warning: Failed to parse selector '{}' for field '{}'",
                selector_str, field_name
            );
        }
    }

    let mut results = Vec::new();

    for container_el in document.select(&container_selector) {
        let mut item_data = HashMap::new();

        for (field_name, selector) in &field_selectors {
            if let Some(target_el) = container_el.select(selector).next() {
                let value = extract_value_by_field_name(field_name, &target_el);
                item_data.insert(field_name.clone(), value);
            } else {
                item_data.insert(field_name.clone(), String::new());
            }
        }

        results.push(item_data);
    }

    // 결과를 serde_json::Value 형태로 변환
    Ok(json!(results))
}

fn extract_value_by_field_name(field_name: &str, element: &ElementRef) -> String {
    let name_lower = field_name.to_lowercase();

    if name_lower.contains("href") || name_lower.contains("link") {
        element
            .value()
            .attr("href")
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            })
    } else if name_lower.contains("src") || name_lower.contains("img") {
        element
            .value()
            .attr("src")
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            })
    } else {
        element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 간소화된 DOM 트리를 로컬 LLM(Ollama)에 전달하여
/// 타깃 목적(target_description)에 적합한 CSS Selector 목록이나 JSON 결과를 얻어옵니다.
pub async fn extract_selectors_via_local_llm(
    simplified_dom: &str,
    target_description: &str,
    ollama_endpoint: Option<&str>,
    model_name: Option<&str>,
) -> Result<SelectorSchema, DynError> {
    // simplified_dom length logging
    println!(
        "🪐 LLM Loading... feed: simplified_dom, length: {}",
        simplified_dom.len()
    );

    // 1. 호스트/포트 설정 (인자 -> 환경변수 -> 기본값 순서로 결정)
    // OLLAMA_HOST 예시: "http://localhost:31434"
    let host_env = env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost".to_string());
    let port_env = env::var("OLLAMA_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(31434);

    let (host, port) = if let Some(endpoint) = ollama_endpoint {
        // ollama_endpoint가 직접 들어온 경우 간단한 파싱 (기본 URL 형태 가정)
        let url = reqwest::Url::parse(endpoint)?;
        let host_str = format!(
            "{}://{}",
            url.scheme(),
            url.host_str().unwrap_or("localhost")
        );
        (host_str, url.port().unwrap_or(31434))
    } else {
        (host_env, port_env)
    };

    let ollama = Ollama::builder().host(host).port(port).build();

    // 2. 모델명 설정 (인자 -> 환경변수 -> 기본값)
    let default_model = env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:14b".to_string());
    let model = model_name.unwrap_or(&default_model).to_string();

    // 3. Prompt 구성 (전달받은 매개변수 바인딩)
    let prompt = format!(
        r#"Below is a simplified HTML DOM tree:
        ---
        {}
        ---
        Task: Find the CSS Selectors to extract a list of items for: "{}"
        Output format: Provide a concise JSON object containing:
        - "container": CSS selector for the repeating item wrapper/card element.
        - "fields": JSON object mapping field names to their relative CSS selectors inside the container.
        Provide ONLY valid JSON."#,
        simplified_dom.trim(),
        target_description.trim()
    );

    // 4. Ollama 요청 및 파싱 (최대 3회 재시도)
    let max_retries = 3;
    let mut last_error: Option<DynError> = None;

    for attempt in 1..=max_retries {
        println!("Attempt {}/{} requesting LLM...", attempt, max_retries);

        // JSON 응답 포맷 강제
        let request =
            GenerationRequest::new(model.clone(), prompt.clone()).format(FormatType::Json);

        match ollama.generate(request).await {
            Ok(ollama_response) => {
                let raw_json = ollama_response.response.trim();

                // Markdown code block이 섞여 들어오는 경우 정제처리
                let cleaned_json = raw_json
                    .strip_prefix("```json")
                    .or_else(|| raw_json.strip_prefix("```"))
                    .unwrap_or(raw_json)
                    .strip_suffix("```")
                    .unwrap_or(raw_json)
                    .trim();

                match serde_json::from_str::<SelectorSchema>(cleaned_json) {
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
            Ok(res) if res.response.is_empty() => {
                eprintln!("Received empty response on attempt {}", attempt);
            }
            Err(e) => {
                eprintln!("Ollama API call failed on attempt {}: {}", attempt, e);
                last_error = Some(Box::new(e));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "Failed to extract valid selectors after retries".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use std::error::Error;
    use std::fs::read_to_string;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_get_simplify_dom() -> Result<(), DynError> {
        // 1. Cargo.toml이 위치한 프로젝트 루트 기준 상대 경로 설정
        let mut test_html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        test_html_path.push("src/bin/scraper/lib/test.html");

        let test_html_content = read_to_string(&test_html_path).map_err(|e| {
            format!(
                "테스트 HTML 파일을 읽을 수 없습니다 ({:?}): {}",
                test_html_path, e
            )
        })?;

        // 2. Headless 브라우저 생성 및 실행
        let (mut browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--headless=new")
                .arg("--no-sandbox")
                .build()?,
        )
        .await?;

        // 3. chromiumoxide 필수: 백그라운드 이벤트 처리 핸들러 태스크 루프
        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    // 경고/에러가 발생해도 백그라운드 루프를 깨뜨리지 않고 계속 진행
                    // eprintln!("Chromium handler event error: {:?}", e);
                }
            }
        });

        let page = browser.new_page("about:blank").await?;

        // 4. HTML 파일 내용을 직접 DOM에 삽입 (navigate 대신 set_content 사용)
        page.set_content(&test_html_content).await?;

        // 5. Simplified DOM tree를 얻어옵니다.
        let simplified_dom_tree = get_simplified_dom_tree(&page).await?;

        // 6. simplified 의 길이가 0이 아닌지 검사하고 0이면 테스트 실패 처리
        assert!(
            simplified_dom_tree.len() > 0,
            "simplified_dom_tree is empty"
        );

        // 7. 원본과 simplified 결과의 길이를 비교합니다. 더 짧아져야 테스트 통과, 아니면 실패.
        assert!(
            simplified_dom_tree.len() <= test_html_content.len(),
            "simplified_dom_tree is longer than test_html_content"
        );

        // 8. 브라우저 종료 및 핸들러 정리
        browser.close().await?;
        handle.abort();

        Ok(())
    }

    #[tokio::test]
    async fn test_get_element() -> DynResult<()> {
        // 1. Cargo.toml이 위치한 프로젝트 루트 기준 상대 경로 설정
        // let mut test_html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // test_html_path.push("src/bin/scraper/lib/test.html");

        let test_html_content = r#"
        <main class="container">
            <article class="card">
                <h2 class="title"><a href="/post/1">첫 번째 글 제목</a></h2>
                <span class="author">작성자A</span>
            </article>
            <article class="card">
                <h2 class="title"><a href="/post/2">두 번째 글 제목</a></h2>
                <span class="author">작성자B</span>
            </article>
        </main>
        "#;

        // 2. Headless 브라우저 생성 및 실행
        let (mut browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--headless=new")
                .arg("--no-sandbox")
                .build()?,
        )
        .await?;

        // 3. chromiumoxide 필수: 백그라운드 이벤트 처리 핸들러 태스크 루프
        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    // 경고/에러가 발생해도 백그라운드 루프를 깨뜨리지 않고 계속 진행
                    eprintln!("Chromium handler event error: {:?}", e);
                }
            }
        });

        let page = browser.new_page("about:blank").await?;

        // 4. HTML 파일 내용을 직접 DOM에 삽입 (navigate 대신 set_content 사용)
        page.set_content(test_html_content).await?;

        // 5. async 함수 내부이므로 block_on 없이 await 직접 사용
        // 5. 메이슨리 그리드 내 첫 번째 href 보유 <a> 태그 지정
        // CSS Selector 방식: href가 존재하는 첫 번째 a 태그 선택
        let selector = "main a[href]";
        let result = get_element_info(&page, selector).await?;

        // 1. 결과(Option) 검증
        let info = result.expect("요소를 찾지 못했습니다 (Result::None)");

        // 2. ExtractedElementInfo 내부 필드 검증
        println!("선택자: {}", info.selector);

        if let Some(text) = &info.text_content {
            println!("추출된 텍스트: {text}");
            assert!(!text.is_empty(), "텍스트 내용이 비어 있습니다.");
        }

        if let Some(html) = &info.outer_html {
            println!("추출된 HTML: {html}");
        }

        // 3. Rect 좌표 검증
        let rect = info.rect.expect("ElementRect 정보를 찾을 수 없습니다.");
        println!("추출된 좌표: {:?}", rect);

        // 좌표값 유효성 단정 (예: 너비와 높이가 0보다 큰지)
        assert!(rect.width > 0.0, "width가 0 이하입니다.");
        assert!(rect.height > 0.0, "height가 0 이하입니다.");

        // 6. 브라우저 종료 및 핸들러 정리
        browser.close().await?;
        handle.abort();

        Ok(())
    }

    #[tokio::test]
    async fn test_extract_selectors_via_local_llm() -> crate::lib::dyn_result::DynResult<()> {
        // 테스트용 샘플 DOM 트리
        let sample_dom = r#"
        <main class="container">
            <article class="card">
                <h2 class="title"><a href="/post/1">첫 번째 글 제목</a></h2>
                <span class="author">작성자A</span>
            </article>
            <article class="card">
                <h2 class="title"><a href="/post/2">두 번째 글 제목</a></h2>
                <span class="author">작성자B</span>
            </article>
        </main>
        "#;

        let target_description = "게시글 목록의 링크(href)와 제목(title), 작성자(author) 추출";

        // 로컬 LLM 함수 호출
        let response_schema = extract_selectors_via_local_llm(
            sample_dom,
            target_description,
            None, // 기본 오버라이드 엔드포인트 사용 안 함 (환경변수/기본값 31434 사용)
            None, // 기본 모델 사용 (qwen2.5:14b)
        )
        .await?;

        let response_string = response_schema.to_string();

        println!("=== LLM Raw Response ===");
        println!("{}", response_string);

        // 응답 텍스트가 JSON 형태인지 검증
        // (LLM이 마크다운 ```json ... ```으로 감싸서 응답하는 경우를 대비해 전처리 후 파싱해볼 수 있습니다)
        let cleaned_json_str = response_string
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let parsed_json: Result<serde_json::Value, _> = serde_json::from_str(cleaned_json_str);

        assert!(
            parsed_json.is_ok(),
            "LLM 응답이 올바른 JSON 형식이 아닙니다: {}",
            response_string
        );

        let json_val = parsed_json?;
        println!("=== Parsed JSON Object ===");
        println!("{:#?}", json_val);

        Ok(())
    }
}
