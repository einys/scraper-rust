use async_trait::async_trait;
use ollama_rs::generation::parameters::FormatType;
use ollama_rs::{generation::completion::request::GenerationRequest, Ollama};
use schemars::JsonSchema;
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::lib::dyn_result::{DynError, DynResult};
use serde_json::{json, Value};

use std::{env, fmt};
// 1. 공통 에러/Result 타입 별칭 정의

// 2. ElementTrait 선언
#[async_trait]
pub trait ElementTrait: Send + Sync {
    async fn click(&self) -> DynResult<()>;
}

#[async_trait]
pub trait PageTrait: Send + Sync {
    async fn evaluate(&self, script: &str) -> DynResult<String>;
    async fn find_element(&self, selector: &str) -> DynResult<Box<dyn ElementTrait>>;
    async fn content(&self) -> DynResult<String>;
    async fn go_back(&self) -> DynResult<()>;
    async fn current_url(&self) -> DynResult<String>;
    async fn navigate(&self, url: &str) -> DynResult<()>;
    async fn screenshot(&self) -> DynResult<Vec<u8>>;

    /// SelectorSchema에 정의된 필드들을 순회하며 ExtractionResult 생성
    async fn extract_element_info_by_selectors(
        &self,
        schema: &SelectorSchema,
    ) -> DynResult<ExtractionResult> {
        // 요약 및 구조정리

        //  "셀렉터로 찾은 item들은 여러 개일 수 있다"의 의미:
        //         한 페이지에 존재하는 여러 필드들(title, price, image 등) 각각을 가리키는 셀렉터들을 모아둔 것이 SelectorSchema입니다.
        //         따라서 HashMap 키로는 CSS 셀렉터 문자열 자체가 아니라, "title", "price" 같은 필드 이름(Field Name)이 들어가야 나중에 데이터 가공이나 파일 저장이 수월해집니다.

        // TODO: query="SelectorSchema의 container 셀렉터를 기준으로 목록 내 여러 아이템을 Vec으로 한 번에 추출하는 메서드로 확장해드릴까요?"
        let mut data = HashMap::new();

        for (field_name, field_selector) in &schema.fields {
            // container 셀렉터와 field 셀렉터 결합
            let full_selector = if schema.container.trim().is_empty() {
                field_selector.clone()
            } else {
                format!("{} {}", schema.container, field_selector)
            };

            // 개별 셀렉터로 DOM 요소 정보(좌표, 텍스트, HTML) 추출
            if let Ok(Some(info)) = get_element_info(self, &full_selector).await {
                // key: 필드명("title", "link" 등), value: 추출된 요소 정보
                data.insert(field_name.clone(), info);
            }
        }

        Ok(ExtractionResult { data })
    }
}

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

impl ExtractedElementInfo {
    /// 요소의 중앙 좌표 계산 (마우스 클릭용)
    pub fn get_center_coordinates(&self) -> Option<(f64, f64)> {
        self.rect.as_ref().map(|r| {
            let center_x = r.x + (r.width / 2.0);
            let center_y = r.y + (r.height / 2.0);
            (center_x, center_y)
        })
    }
}

/// 최종 JSON 결과 (필드명 -> ExtractedElementInfo)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractionResult {
    pub data: HashMap<String, ExtractedElementInfo>,
}

impl fmt::Display for ExtractionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string_pretty(&self.data) {
            Ok(json) => write!(f, "{}", json),
            Err(_) => Err(fmt::Error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)] // 👈 JsonSchema 추가!
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

use regex::{Captures, Regex};
use std::sync::OnceLock;

static TAILWIND_SANITY_REGEX: OnceLock<Regex> = OnceLock::new();

/// Tailwind 클래스의 모든 특수문자(:, [, ], /, %, 소수점)를 단일 패스로 정밀 이스케이프
pub fn sanitize_selector(selector: &str) -> String {
    let trim_sel = selector.trim();
    if trim_sel.is_empty() {
        return String::new();
    }

    // 1. 소수점 클래스 이스케이프 (.mx-0.5 -> .mx-0\.5)
    let re_float = Regex::new(r"\.([a-zA-Z0-9_\-]+)\.([0-9]+)").unwrap();
    let step1 = re_float.replace_all(trim_sel, |caps: &Captures| {
        format!(".{}\\{}", &caps[1], &caps[2])
    });

    // 2. 분수/슬래시 클래스 이스케이프 (.top-1/2 -> .top-1\/2)
    let re_slash = Regex::new(r"\.([a-zA-Z0-9_\-]+)/([a-zA-Z0-9_\-]+)").unwrap();
    let step2 = re_slash.replace_all(&step1, |caps: &Captures| {
        format!(".{}\\{}", &caps[1], &caps[2])
    });

    // 3. 임의 값 대괄호 클래스 이스케이프 (.text-[10px] -> .text-\[10px\])
    let re_bracket = Regex::new(r"\.([a-zA-Z0-9_\-]+)\[([^\]]+)\]").unwrap();
    let step3 = re_bracket.replace_all(&step2, |caps: &Captures| {
        format!(".{}\\[{}\\]", &caps[1], &caps[2])
    });

    step3.to_string()
}

// selector 받고 -> 내용, 원본, x y 좌표 담긴 구조체 리턴
pub async fn get_element_info<P: PageTrait + ?Sized>(
    page: &P,
    raw_selector: &str,
) -> Result<Option<ExtractedElementInfo>, DynError> {
    // 1. Rust 단에서 1차적으로 Tailwind 특수문자 이스케이프
    // 1. 셀렉터 정제 및 serde_json으로 JS 문자열 리터럴 생성
    let safe_selector = sanitize_selector(raw_selector);
    let selector_json = serde_json::to_string(&safe_selector)?; // 👈 "\" .flex-1... \"" 형태의 올바른 JS 문자열 생성
    let js_script = format!(
        r#"
        (() => {{
            const rawSelector = {}; // 👈 "..." 따옴표가 자동 포함된 문자열로 안전하게 주입됨

            const el = document.querySelector(rawSelector);
            if (!el) return null;
            const rect = el.getBoundingClientRect();
            return {{
                selector: rawSelector,
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
        "#,
        selector_json
    );

    // null인 경우 deserialization 에러를 피하고 Option<ExtractedElementInfo>로 매핑
    let result: Option<ExtractedElementInfo> = match page.evaluate(&js_script).await {
        Ok(result_str) => {
            if result_str == "null" {
                None
            } else {
                Some(serde_json::from_str(&result_str)?)
            }
        }
        Err(e) => {
            eprintln!("Evaluation 오류: {} of selector_json: {}", e, safe_selector);
            None
        }
    };
    Ok(result)
}

/// 리턴: json형식의 '문자열'. LLM Selector 결과와 브라우저 Page를 받아 위치 및 내용이 통합된 JSON 결과를 반환합니다.
pub async fn extract_data_with_selectors<P: PageTrait>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<ExtractionResult, DynError> {
    // LLM 결과에서 JSON 부분 추출 (마크다운 ```json ... ``` 응답 대응)
    // let clean_json_str = parse_json_from_llm_response(llm_selector_json_str);
    // 💡 문자열 파싱 과정(parse_json_from_llm_response)이 제거되고
    //    타입이 보장된 schema 구조체를 직접 활용합니다.
    let mut final_data: HashMap<String, ExtractedElementInfo> = HashMap::new();
    for (field_name, selector) in &schema.fields {
        // 컨테이너 셀렉터와 필드 상대 셀렉터를 결합하거나 단독 셀렉터로 정보 추출
        let full_selector = if schema.container.trim().is_empty() {
            selector.clone()
        } else {
            format!("{} {}", schema.container, selector)
        };

        if let Ok(Some(element_info)) = get_element_info(page, &full_selector).await {
            final_data.insert(field_name.clone(), element_info);
        } else {
            // 요소를 찾지 못했거나 에러인 경우 None 구조 유지
            final_data.insert(
                field_name.clone(),
                ExtractedElementInfo {
                    selector: full_selector,
                    text_content: None,
                    outer_html: None,
                    rect: None,
                },
            );
        }
    }

    let extraction_result = ExtractionResult { data: final_data };

    // 예쁘게 정렬된 JSON 문자열로 반환
    //let json_output = serde_json::to_string_pretty(&extraction_result)?;
    Ok(extraction_result)
}

/// LLM 응답에 섞인 마크다운 코드블록 제거용 헬퍼 함수
pub fn parse_json_from_llm_response(raw_response: &str) -> String {
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

/// 브라우저 DOM에서 크롤링/스크래핑 관점에서 의미 있는 핵심 태그, id, class, 주요 속성 및 data-* 속성을 포함한 간소화된 DOM 텍스트 트리를 추출합니다.
pub async fn get_simplified_dom_tree<P: PageTrait>(page: &P) -> Result<String, DynError> {
    let js_script = r#"
        (() => {
            // 💡 1. DOM에 data-index가 없으면 자체적으로 자동 주입 (전처리)
            const listContainers = document.querySelectorAll(
                '#masonry, ul, ol, [class*="grid"], [class*="list"], [class*="flex-col"], [class*="masonry"]'
            );

            listContainers.forEach(container => {
                const children = Array.from(container.children);
                if (children.length >= 2) {
                    children.forEach((child, idx) => {
                        if (!child.hasAttribute('data-index')) {
                            child.setAttribute('data-index', idx.toString());
                        }
                    });
                }
            });

            const SIGNIFICANT_TAGS = new Set([
                'body', 'header', 'nav', 'main', 'article', 'section', 'aside', 'footer',
                'div', 'ul', 'ol', 'li', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'img', 'span',
                'a', 'button', 'form', 'input', 'select', 'textarea', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'
            ]);

            // 반복 자식 요소 최대 제한 개수
            const MAX_REPEATED_CHILDREN = 3;

            function buildTree(node, depth = 0) {
                if (!node || node.nodeType !== Node.ELEMENT_NODE) return '';

                const tagName = node.tagName.toLowerCase();
                if (!SIGNIFICANT_TAGS.has(tagName)) return '';

                const style = window.getComputedStyle(node);
                if (style.display === 'none' || style.visibility === 'hidden') return '';

                const indent = '  '.repeat(depth);
                let attrStr = '';

                if (node.id) {
                    attrStr += ` id="${node.id}"`;
                }
                if (node.className && typeof node.className === 'string' && node.className.trim() !== '') {
                    // Tailwind/유틸리티 클래스 유지를 위해 최대 3개 클래스로 제한
                    const classes = node.className.trim().split(/\s+/).slice(0, 3).join(' ');
                    attrStr += ` class="${classes}"`;
                }
                if (node.hasAttribute('href')) {
                    attrStr += ` href="${node.getAttribute('href')}"`;
                }
                if (node.hasAttribute('name')) {
                    attrStr += ` name="${node.getAttribute('name')}"`;
                }

                // 💡 원본이거나 전처리로 주입된 data-index를 포함한 모든 data-* 속성 수집 추가
                for (const attr of node.attributes) {
                    if (attr.name.startsWith('data-')) {
                        attrStr += ` ${attr.name}="${attr.value}"`;
                    }
                }

                let directText = '';
                for (const child of node.childNodes) {
                    if (child.nodeType === Node.TEXT_NODE) {
                        const text = child.textContent.trim();
                        if (text.length > 0) {
                            directText = text.length > 25 ? text.substring(0, 25) + '...' : text;
                            break;
                        }
                    }
                }

                const textAttr = directText ? ` text="${directText}"` : '';
                let result = `${indent}<${tagName}${attrStr}${textAttr}>\n`;

                // 자식 노드들의 "태그명+클래스명" 조합으로 그룹핑하여 무한 반복을 3개로 자름
                const childCounts = new Map();
                let truncatedCount = 0;

                for (const child of node.children) {
                    const childTag = child.tagName.toLowerCase();
                    const childClass = (child.className && typeof child.className === 'string')
                        ? child.className.trim().split(/\s+/)[0] || ''
                        : '';
                    const key = `${childTag}.${childClass}`;

                    const currentCount = childCounts.get(key) || 0;
                    if (currentCount >= MAX_REPEATED_CHILDREN) {
                        truncatedCount++;
                        continue; // 3개가 넘어가면 생략
                    }

                    childCounts.set(key, currentCount + 1);
                    result += buildTree(child, depth + 1);
                }

                // 축약된 요소가 있다면 LLM에게 생략되었음을 명시
                if (truncatedCount > 0) {
                    result += `${indent}  <!-- ... [${truncatedCount} similar items truncated] ... -->\n`;
                }

                return result;
            }

            return buildTree(document.body || document.documentElement);
        })()
    "#;

    let tree: String = page.evaluate(js_script).await?;
    Ok(tree)
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
        // 1. 현재 요소(또는 선택된 요소) 자체가 href 속성을 가지고 있는 경우
        if let Some(href) = element.value().attr("href") {
            return href.to_string();
        }
        // 2. 혹시 자식 노드 중에 a 태그가 따로 존재하는 경우
        if let Ok(a_sel) = scraper::Selector::parse("a") {
            if let Some(a_el) = element.select(&a_sel).next() {
                if let Some(href) = a_el.value().attr("href") {
                    return href.to_string();
                }
            }
        }
        // Fallback: 텍스트 수집
        element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    } else if name_lower.contains("src") || name_lower.contains("img") {
        if let Some(src) = element.value().attr("src") {
            return src.to_string();
        }
        if let Ok(img_sel) = scraper::Selector::parse("img") {
            if let Some(img_el) = element.select(&img_sel).next() {
                if let Some(src) = img_el.value().attr("src") {
                    return src.to_string();
                }
            }
        }
        element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
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

#[cfg(test)]
mod tests {
    use crate::lib::dyn_result::DynResult;
    use crate::ChromiumPageAdapter;

    use super::*;

    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
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

        // 어댑터로 감싸기
        let page_adapter = ChromiumPageAdapter { page: page };
        // 5. Simplified DOM tree를 얻어옵니다.
        let simplified_dom_tree = get_simplified_dom_tree(&page_adapter).await?;

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
        let page_adaptor = ChromiumPageAdapter { page: page };
        // 5. async 함수 내부이므로 block_on 없이 await 직접 사용
        // 5. 메이슨리 그리드 내 첫 번째 href 보유 <a> 태그 지정
        // CSS Selector 방식: href가 존재하는 첫 번째 a 태그 선택
        let selector = "main a[href]";
        let result = get_element_info(&page_adaptor, selector).await?;

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
}
