use async_trait::async_trait;
use regex::{Captures, Regex};
use schemars::JsonSchema;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;

use crate::lib::dyn_result::{DynError, DynResult};

// ============================================================================
// Core Traits
// ============================================================================

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

    /// SelectorSchema에 정의된 필드들을 순회하며 ExtractedData 생성
    async fn collect_schema_data(
        &self,
        schema: &SelectorSchema,
    ) -> DynResult<ExtractedContentWrapper> {
        let container_sel = schema.container.trim();

        if !container_sel.is_empty() {
            let rows = get_container_rows(self, schema).await?;
            println!(
                "Successfully Extracted {} rows from list container.",
                rows.len()
            );

            Ok(ExtractedContentWrapper {
                data: ExtractedContent::List(rows),
            })
        } else {
            let mut data = HashMap::new();

            for (field_name, field_selector) in &schema.fields {
                if let Ok(Some(info)) = get_element_info(self, field_selector).await {
                    data.insert(field_name.clone(), info);
                } else {
                    data.insert(
                        field_name.clone(),
                        ElementRefInfo {
                            selector: field_selector.clone(),
                            text_content: None,
                            rect: None,
                            href: None,
                            src: None,
                        },
                    );
                }
            }

            Ok(ExtractedContentWrapper {
                data: ExtractedContent::Single(data),
            })
        }
    }
}

// ============================================================================
// Data Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Selector 단위 요소 정보 (위치 + 원본 내용)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementRefInfo {
    pub selector: String,
    pub text_content: Option<String>,
    pub href: Option<String>, // 👈 링크 이동용 URL
    pub src: Option<String>,  // 👈 이미지 주소
    pub rect: Option<ElementRect>,
}
impl ElementRefInfo {
    pub fn get_center_coordinates(&self) -> Option<(f64, f64)> {
        self.rect.as_ref().map(|r| {
            let center_x = r.x + (r.width / 2.0);
            let center_y = r.y + (r.height / 2.0);
            (center_x, center_y)
        })
    }
}

pub type ExtractedRow = HashMap<String, Option<ElementRefInfo>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtractedContent {
    /// 상세 페이지 등 단일 추출 데이터
    Single(HashMap<String, ElementRefInfo>),
    /// 피드/목록 페이지 추출 데이터
    List(Vec<ExtractedRow>),
}

impl Default for ExtractedContent {
    fn default() -> Self {
        ExtractedContent::List(Vec::new())
    }
}

impl ExtractedContent {
    pub fn is_empty(&self) -> bool {
        match self {
            ExtractedContent::Single(map) => map.is_empty(),
            ExtractedContent::List(vec) => vec.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            ExtractedContent::Single(map) => map.len(),
            ExtractedContent::List(vec) => vec.len(),
        }
    }
}

/// 최종 수집 결과 wrapper
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedContentWrapper {
    pub data: ExtractedContent,
}

use std::ops::Deref;

impl Deref for ExtractedContentWrapper {
    type Target = ExtractedContent;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl fmt::Display for ExtractedContentWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string_pretty(&self.data) {
            Ok(json) => write!(f, "{}", json),
            Err(_) => Err(fmt::Error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectorSchema {
    pub container: String,
    pub fields: HashMap<String, String>,
}

impl fmt::Display for SelectorSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string_pretty(self) {
            Ok(json_str) => write!(f, "{}", json_str),
            Err(_) => Err(fmt::Error),
        }
    }
}

// ============================================================================
// Utilities & Scrapers
// ============================================================================

pub fn sanitize_selector(selector: &str) -> String {
    let trim_sel = selector.trim();
    if trim_sel.is_empty() {
        return String::new();
    }

    let re_float = Regex::new(r"\.([a-zA-Z0-9_\-]+)\.([0-9]+)").unwrap();
    let step1 = re_float.replace_all(trim_sel, |caps: &Captures| {
        format!(".{}\\{}", &caps[1], &caps[2])
    });

    let re_slash = Regex::new(r"\.([a-zA-Z0-9_\-]+)/([a-zA-Z0-9_\-]+)").unwrap();
    let step2 = re_slash.replace_all(&step1, |caps: &Captures| {
        format!(".{}\\{}", &caps[1], &caps[2])
    });

    let re_bracket = Regex::new(r"\.([a-zA-Z0-9_\-]+)\[([^\]]+)\]").unwrap();
    let step3 = re_bracket.replace_all(&step2, |caps: &Captures| {
        format!(".{}\\[{}\\]", &caps[1], &caps[2])
    });

    step3.to_string()
}

pub async fn get_element_info<P: PageTrait + ?Sized>(
    page: &P,
    raw_selector: &str,
) -> Result<Option<ElementRefInfo>, DynError> {
    let safe_selector = sanitize_selector(raw_selector);
    let selector_json = serde_json::to_string(&safe_selector)?;
    let js_script = format!(
        r#"
        (() => {{
            const rawSelector = {};

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

    let result: Option<ElementRefInfo> = match page.evaluate(&js_script).await {
        Ok(result_str) => {
            if result_str == "null" || result_str.is_empty() {
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

pub async fn get_container_rows<P: PageTrait + ?Sized>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<Vec<HashMap<String, Option<ElementRefInfo>>>, DynError> {
    let container_json = serde_json::to_string(&sanitize_selector(&schema.container))?;

    let mut sanitized_fields = HashMap::new();
    for (k, v) in &schema.fields {
        sanitized_fields.insert(k.clone(), sanitize_selector(v));
    }
    let fields_json = serde_json::to_string(&sanitized_fields)?;

    let js_script = format!(
        r#"
        (() => {{
            const toAbsUrl = (urlStr) => {{
                if (!urlStr) return null;
                try {{
                    return new URL(urlStr, document.baseURI).href;
                }} catch (e) {{
                    return urlStr;
                }}
            }};

            const containerSel = {container_json};
            const fieldsMap = {fields_json};

            const containers = document.querySelectorAll(containerSel);
            if (containers.length === 0) return JSON.stringify([]);

            const results = Array.from(containers).map(parent => {{
                const item = {{}};
                for (const [key, rawFieldSel] of Object.entries(fieldsMap)) {{
                    if (!rawFieldSel) continue;

                    let targetEl = null;
                    const trimmedSel = rawFieldSel.trim();

                    if (trimmedSel === '.' || trimmedSel === ':scope' || (trimmedSel === 'a' && parent.tagName.toLowerCase() === 'a')) {{
                        targetEl = parent;
                    }} else {{
                        const selectorToUse = trimmedSel.startsWith('>')
                            ? `:scope ${{trimmedSel}}`
                            : trimmedSel;
                        targetEl = parent.querySelector(selectorToUse);
                    }}

                    if (targetEl) {{
                        const rect = targetEl.getBoundingClientRect();
                        const hrefAttr = targetEl.getAttribute('href') || targetEl.closest('a')?.getAttribute('href') || targetEl.querySelector('a')?.getAttribute('href');
                        const srcAttr = targetEl.getAttribute('src') || targetEl.getAttribute('data-src') || targetEl.querySelector('img')?.getAttribute('src');
                        const textContent = (targetEl.innerText || targetEl.textContent || '').trim();

                        item[key] = {{
                            selector: trimmedSel,
                            text_content: textContent.length > 0 ? textContent : null,
                            href: toAbsUrl(hrefAttr),
                            src: toAbsUrl(srcAttr),
                            rect: {{
                                x: rect.x + window.scrollX,
                                y: rect.y + window.scrollY,
                                width: rect.width,
                                height: rect.height
                            }}
                        }};
                    }} else {{
                        item[key] = null;
                    }}
                }}
                return item;
            }});

            return JSON.stringify(results);
        }})()
        "#
    );

    let result_str = page.evaluate(&js_script).await?;
    let items: Vec<HashMap<String, Option<ElementRefInfo>>> =
            serde_json::from_str(&result_str).map_err(|e| {
                format!(
                    "Failed to deserialize JSON to Vec<HashMap<String, Option<ElementRefInfo>>>: {}\nRaw JSON Output:\n{}",
                    e, result_str
                )
            })?;
    Ok(items)
}

pub async fn extract_by_selector_schema<P: PageTrait>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<ExtractedContentWrapper, DynError> {
    page.collect_schema_data(schema).await
}

/// SelectorSchema 기반으로 화면상에 실제 노출된(유효한) 요소 정보 및 절대 경로 URL을 검증/수집합니다.
pub async fn locate_selector_schema_elements<P: PageTrait + ?Sized>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<ExtractedContentWrapper, DynError> {
    let container_sel = schema.container.trim();

    // 공통 JS 헬퍼 유틸리티 함수 정의
    let js_helpers = r#"
        const toAbsUrl = (urlStr) => {
            if (!urlStr) return null;
            try {
                return new URL(urlStr, document.baseURI).href;
            } catch (e) {
                return urlStr;
            }
        };

        const isElementVisible = (el) => {
            if (!el) return false;
            const rect = el.getBoundingClientRect();
            const style = window.getComputedStyle(el);
            return rect.width > 0 &&
                   rect.height > 0 &&
                   style.visibility !== 'hidden' &&
                   style.display !== 'none' &&
                   style.opacity !== '0';
        };

        const extractRefInfo = (el, rawSel) => {
            const rect = el.getBoundingClientRect();

            // href: 자신 또는 상위/하위 <a> 태그 탐색
            const hrefAttr = el.getAttribute('href') || el.closest('a')?.getAttribute('href') || el.querySelector('a')?.getAttribute('href');

            // src: 자신 또는 하위 <img> 태그 탐색
            const srcAttr = el.getAttribute('src') || el.getAttribute('data-src') || el.querySelector('img')?.getAttribute('src');

            const textContent = (el.innerText || el.textContent || '').trim();

            return {
                selector: rawSel,
                text_content: textContent.length > 0 ? textContent : null,
                href: toAbsUrl(hrefAttr),
                src: toAbsUrl(srcAttr),
                rect: {
                    x: rect.x + window.scrollX,
                    y: rect.y + window.scrollY,
                    width: rect.width,
                    height: rect.height
                }
            };
        };
    "#;

    if container_sel.is_empty() {
        // ---------------------------------------------------------------------
        // 1. Single 모드 (상세 페이지 / 단일 요소)
        // ---------------------------------------------------------------------
        let mut sanitized_fields = HashMap::new();
        for (k, v) in &schema.fields {
            sanitized_fields.insert(k.clone(), sanitize_selector(v));
        }
        let fields_json = serde_json::to_string(&sanitized_fields)?;

        let js_script = format!(
            r#"
            (() => {{
                {js_helpers}

                const fieldsMap = {fields_json};
                const result = {{}};

                for (const [key, rawSel] of Object.entries(fieldsMap)) {{
                    if (!rawSel) continue;
                    const el = document.querySelector(rawSel);
                    if (isElementVisible(el)) {{
                        result[key] = extractRefInfo(el, rawSel);
                    }}
                }}
                return JSON.stringify(result);
            }})()
            "#
        );

        let result_str: String = page.evaluate(&js_script).await?;
        let single_map: HashMap<String, ElementRefInfo> = serde_json::from_str(&result_str)?;

        Ok(ExtractedContentWrapper {
            data: ExtractedContent::Single(single_map),
        })
    } else {
        // ---------------------------------------------------------------------
        // 2. List 모드 (피드 / 목록 페이지)
        // ---------------------------------------------------------------------
        let container_json = serde_json::to_string(&sanitize_selector(container_sel))?;

        let mut sanitized_fields = HashMap::new();
        for (k, v) in &schema.fields {
            sanitized_fields.insert(k.clone(), sanitize_selector(v));
        }
        let fields_json = serde_json::to_string(&sanitized_fields)?;

        let js_script = format!(
            r#"
            (() => {{
                {js_helpers}

                const containerSel = {container_json};
                const fieldsMap = {fields_json};

                const containers = document.querySelectorAll(containerSel);
                if (containers.length === 0) return JSON.stringify([]);

                const results = Array.from(containers).map(parent => {{
                    const item = {{}};

                    for (const [key, rawFieldSel] of Object.entries(fieldsMap)) {{
                        if (!rawFieldSel) continue;

                        let targetEl = null;
                        const trimmedSel = rawFieldSel.trim();

                        if (trimmedSel === '.' || trimmedSel === ':scope' || (trimmedSel === 'a' && parent.tagName.toLowerCase() === 'a')) {{
                            targetEl = parent;
                        }} else {{
                            const selectorToUse = trimmedSel.startsWith('>')
                                ? `:scope ${{trimmedSel}}`
                                : trimmedSel;
                            targetEl = parent.querySelector(selectorToUse);
                        }}

                        if (isElementVisible(targetEl)) {{
                            item[key] = extractRefInfo(targetEl, trimmedSel);
                        }}
                    }}
                    return item;
                }});

                return JSON.stringify(results);
            }})()
            "#
        );

        let result_str: String = page.evaluate(&js_script).await?;
        let list_vec: Vec<HashMap<String, Option<ElementRefInfo>>> =
            serde_json::from_str(&result_str)?;

        Ok(ExtractedContentWrapper {
            data: ExtractedContent::List(list_vec),
        })
    }
}

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
// DOM Simplifier & Offline Parser
// ============================================================================

pub async fn get_simplified_dom_tree<P: PageTrait>(page: &P) -> Result<String, DynError> {
    let js_script = r#"
        (() => {
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
                    const classes = node.className.trim().split(/\s+/).slice(0, 3).join(' ');
                    attrStr += ` class="${classes}"`;
                }
                if (node.hasAttribute('href')) {
                    attrStr += ` href="${node.getAttribute('href')}"`;
                }
                if (node.hasAttribute('name')) {
                    attrStr += ` name="${node.getAttribute('name')}"`;
                }

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
                        continue;
                    }

                    childCounts.set(key, currentCount + 1);
                    result += buildTree(child, depth + 1);
                }

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

pub fn parse_html_rows(raw_html: &str, schema: &SelectorSchema) -> Result<Value, DynError> {
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
                let value = parse_field_value(field_name, &target_el);
                item_data.insert(field_name.clone(), value);
            } else {
                item_data.insert(field_name.clone(), String::new());
            }
        }

        results.push(item_data);
    }

    Ok(json!(results))
}

fn parse_field_value(field_name: &str, element: &ElementRef) -> String {
    let name_lower = field_name.to_lowercase();

    if name_lower.contains("href") || name_lower.contains("link") {
        if let Some(href) = element.value().attr("href") {
            return href.to_string();
        }
        if let Ok(a_sel) = scraper::Selector::parse("a") {
            if let Some(a_el) = element.select(&a_sel).next() {
                if let Some(href) = a_el.value().attr("href") {
                    return href.to_string();
                }
            }
        }
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib::dyn_result::DynResult;
    use crate::ChromiumPageAdapter;

    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use std::fs::read_to_string;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_get_simplify_dom() -> Result<(), DynError> {
        let mut test_html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        test_html_path.push("src/bin/scraper/lib/test.html");

        let test_html_content = read_to_string(&test_html_path).map_err(|e| {
            format!(
                "테스트 HTML 파일을 읽을 수 없습니다 ({:?}): {}",
                test_html_path, e
            )
        })?;

        let (mut browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--headless=new")
                .arg("--no-sandbox")
                .build()?,
        )
        .await?;

        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(_e) = event {}
            }
        });

        let page = browser.new_page("about:blank").await?;
        page.set_content(&test_html_content).await?;

        let page_adapter = ChromiumPageAdapter { page };
        let simplified_dom_tree = get_simplified_dom_tree(&page_adapter).await?;

        assert!(
            !simplified_dom_tree.is_empty(),
            "simplified_dom_tree is empty"
        );
        assert!(
            simplified_dom_tree.len() <= test_html_content.len(),
            "simplified_dom_tree is longer than test_html_content"
        );

        browser.close().await?;
        handle.abort();

        Ok(())
    }

    #[tokio::test]
    async fn test_get_element() -> DynResult<()> {
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

        let (mut browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--headless=new")
                .arg("--no-sandbox")
                .build()?,
        )
        .await?;

        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    eprintln!("Chromium handler event error: {:?}", e);
                }
            }
        });

        let page = browser.new_page("about:blank").await?;
        page.set_content(test_html_content).await?;
        let page_adapter = ChromiumPageAdapter { page };

        let selector = "main a[href]";
        let result = get_element_info(&page_adapter, selector).await?;
        let info = result.expect("요소를 찾지 못했습니다");

        println!("선택자: {}", info.selector);

        if let Some(text) = &info.text_content {
            println!("추출된 텍스트: {text}");
            assert!(!text.is_empty(), "텍스트 내용이 비어 있습니다.");
        }

        let rect = info.rect.expect("ElementRect 정보를 찾을 수 없습니다.");
        println!("추출된 좌표: {:?}", rect);

        assert!(rect.width > 0.0, "width가 0 이하입니다.");
        assert!(rect.height > 0.0, "height가 0 이하입니다.");

        browser.close().await?;
        handle.abort();

        Ok(())
    }
}
