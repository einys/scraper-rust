use chromiumoxide::Page;
use ollama_rs::{generation::completion::request::GenerationRequest, Ollama};
use serde::Deserialize;
use std::error::Error;
use tokio::test;

#[derive(Deserialize, Debug)]
struct ElementRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

async fn get_element_rect(
    page: &Page,
    selector: &str,
) -> Result<Option<ElementRect>, Box<dyn Error>> {
    // JS 문자열 이스케이프 및 안전성을 위해 JSON.stringify 사용
    let selector_json = serde_json::to_string(selector)?;

    let js_script = format!(
        r#"
        (() => {{
            const el = document.querySelector({selector_json});
            if (!el) return null;
            const rect = el.getBoundingClientRect();
            return {{
                x: rect.x + window.scrollX,
                y: rect.y + window.scrollY,
                width: rect.width,
                height: rect.height
            }};
        }})()
        "#
    );

    // null인 경우 deserialization 에러를 피하고 Option<ElementRect>로 매핑
    let result: Option<ElementRect> = page.evaluate(js_script).await?.into_value()?;
    // Option<ElementRect> 반환):
    // serde가 JS의 null을 Rust의 None으로 자연스럽게 변환합니다.
    // 요소를 찾지 못한 상황이 정상적인 흐름(Ok(None))으로 처리되어, match나 if let Some() 문으로 깔끔하게 대응할 수 있습니다.
    Ok(result)
}

// ============================================================================
// 1. DOM 축약 간소화 트리 생성
// ============================================================================

/// 브라우저 DOM에서 크롤링/스크래핑 관점에서 의미 있는 핵심 태그, id, class, 주요 속성만 남긴 간소화된 DOM 텍스트 트리를 추출합니다.
pub async fn get_simplified_dom_tree(page: &Page) -> Result<String, Box<dyn Error>> {
    let js_script = r#"
    (() => {
        // 의미 있는 주요 태그 목록 (시각적/구조적 레이아웃 요소 위주)
        const SIGNIFICANT_TAGS = new Set([
            'HEADER', 'NAV', 'MAIN', 'ARTICLE', 'SECTION', 'ASIDE', 'FOOTER',
            'DIV', 'UL', 'OL', 'LI', 'TABLE', 'THEAD', 'TBODY', 'TR', 'TH', 'TD',
            'A', 'BUTTON', 'FORM', 'INPUT', 'SELECT', 'TEXTAREA', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6'
        ]);

        function buildTree(node, depth = 0) {
            if (!node || node.nodeType !== Node.ELEMENT_NODE) return '';

            const tagName = node.tagName.toUpperCase();
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
            let result = `${indent}<${tagName.toLowerCase()}${attrStr}${textAttr}>\n`;

            for (const child of node.children) {
                result += buildTree(child, depth + 1);
            }

            return result;
        }

        return buildTree(document.body || document.documentElement);
    })()
    "#;

    let tree: String = page.evaluate(js_script).await?.into_value()?;
    Ok(tree)
}

use std::env;

/// 간소화된 DOM 트리를 로컬 LLM(Ollama)에 전달하여
/// 타깃 목적(target_description)에 적합한 CSS Selector 목록이나 JSON 결과를 얻어옵니다.
pub async fn extract_selectors_via_local_llm(
    simplified_dom: &str,
    target_description: &str,
    ollama_endpoint: Option<&str>,
    model_name: Option<&str>,
) -> Result<String, Box<dyn Error>> {
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
        Task: Find the best CSS Selectors for the following request: "{}"
        Output format: Provide a concise JSON object containing field names and their corresponding CSS selectors without any unnecessary explanation."#,
        simplified_dom.trim(),
        target_description.trim()
    );

    // 4. Ollama 요청 및 응답 반환
    let request = GenerationRequest::new(model, prompt);
    let ollama_response = ollama.generate(request).await?;

    Ok(ollama_response.response)
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
    async fn test_get_element_rect() -> Result<(), Box<dyn Error>> {
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
        let result = get_element_rect(&page, selector).await?;

        // 요소가 존재하고 Option 내부에 값이 있는지 검증
        assert!(result.is_some(), "요소를 찾을 수 없습니다.");

        let rect = result.unwrap();
        println!("추출된 좌표: {:?}", rect);

        // 6. 브라우저 종료 및 핸들러 정리
        browser.close().await?;
        handle.abort();

        Ok(())
    }

    #[tokio::test]
    async fn test_extract_selectors_via_local_llm() -> Result<(), Box<dyn Error>> {
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
        let response_string = extract_selectors_via_local_llm(
            sample_dom,
            target_description,
            None, // 기본 오버라이드 엔드포인트 사용 안 함 (환경변수/기본값 31434 사용)
            None, // 기본 모델 사용 (qwen2.5:14b)
        )
        .await?;

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
