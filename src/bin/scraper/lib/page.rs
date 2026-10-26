use async_trait::async_trait;
use regex::{Captures, Regex};
use schemars::JsonSchema;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::time::Instant;
use std::{collections::HashMap, time::Duration};

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
    async fn click_at_point(&self, x: f64, y: f64) -> DynResult<()>;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectorSchema {
    /// 필드명과 원본 DOM에서 바로 querySelectorAll로 수집할 수 있는 Full CSS Selector
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

    // group/link 같은 Tailwind 슬래시 이스케이프 (\/)
    let re_slash_class = Regex::new(r"([a-zA-Z0-9_\-]+)/([a-zA-Z0-9_\-]+)").unwrap();
    let step4 = re_slash_class.replace_all(&step3, |caps: &Captures| {
        format!("{}\\{}", &caps[1], &caps[2])
    });

    step4.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TargetItem {
    pub selector: String,
    pub target_url: Option<String>, // 중복 체크 기준 (href 절대 경로)
    pub center_x: f64,
    pub center_y: f64,
}

impl std::fmt::Display for TargetItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Selector: {}, Target URL: {:?}, Center X: {}, Center Y: {}",
            self.selector, self.target_url, self.center_x, self.center_y
        )
    }
}

impl TargetItem {
    pub fn new(selector: String, target_url: Option<String>, center_x: f64, center_y: f64) -> Self {
        TargetItem {
            selector,
            target_url,
            center_x,
            center_y,
        }
    }
}

pub async fn get_target_items<P: PageTrait>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<Vec<TargetItem>, DynError> {
    let schema_json = serde_json::to_string(schema)?;

    println!("🐏 Fetching target items with schema: {}", schema_json);

    let js_code = format!(
        r#"
        (() => {{
            const schema = {};
            const results = [];
            const visitedSet = new Set();

            for (const [field_name, raw_selector] of Object.entries(schema.fields || {{}})) {{
                if (!raw_selector) continue;
                try {{
                    const elements = document.querySelectorAll(raw_selector);
                    elements.forEach((el) => {{
                        // 💡 1. 자식 요소(span, img 등)가 선택된 경우 부모 <a> 태그를 찾아 실제 클릭 타겟으로 승격
                        const anchorEl = el.tagName === 'A' ? el : el.closest('a');
                        const targetEl = anchorEl || el; // <a> 태그가 없으면 원본 요소 유지


                        let target_url = '';
                        if (anchorEl) {{
                            let href = anchorEl.getAttribute('href');
                            if (!href && el.closest('a')) {{ // href 가 없다면 가까운 <a> 태그를 찾아 실제 클릭 타겟으로 승격
                                href = el.closest('a').getAttribute('href');
                            }}
                            if (href) {{
                                try {{
                                    target_url = new URL(href, window.location.href).href;
                                }} catch(e) {{
                                    target_url = href;
                                }}
                            }}
                        }}

                        // 💡 2. 좌표 계산을 승격된 targetEl(실제 <a> 태그)의 bounding box 기준으로 실행
                        const rect = targetEl.getBoundingClientRect();

                        // 화면 밖이거나 크기가 0인 노드 필터링
                        if (rect.width === 0 && rect.height === 0) return;

                        const key = target_url
                            ? (target_url + '|' + Math.round(rect.left) + '|' + Math.round(rect.top))
                            : (raw_selector + '|' + Math.round(rect.left) + '|' + Math.round(rect.top));

                        if (!visitedSet.has(key)) {{
                            visitedSet.add(key);
                            results.push({{
                                // anchorEl이 존재하면 'a' 태그 셀렉터 우선 적용
                                selector: anchorEl ? (raw_selector + " (a)") : raw_selector,
                                target_url: target_url || null,
                                center_x: rect.left + rect.width / 2,
                                center_y: rect.top + rect.height / 2,
                                text: targetEl.innerText ? targetEl.innerText.trim().slice(0, 100) : ''
                            }});
                        }}
                    }});
                }} catch(e) {{
                    console.error('Selector error:', raw_selector, e);
                }}
            }}
            return results;
        }})()
        "#,
        schema_json
    );

    let eval_res = page.evaluate(&js_code).await?;
    let items: Vec<TargetItem> = serde_json::from_str(&eval_res)?;
    Ok(items)
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

pub async fn inject_visual_cursor<P: PageTrait>(page: &P) -> DynResult<()> {
    let js_code = r#"
    (() => {
        if (document.getElementById('agent-visual-cursor')) return;

        // 1. 빨간색 마우스 커서 포인터 생성
        const cursor = document.createElement('div');
        cursor.id = 'agent-visual-cursor';
        cursor.style.position = 'fixed';
        cursor.style.top = '0';
        cursor.style.left = '0';
        cursor.style.width = '14px';
        cursor.style.height = '14px';
        cursor.style.backgroundColor = 'rgba(255, 0, 0, 0.85)';
        cursor.style.border = '2px solid white';
        cursor.style.borderRadius = '50%';
        cursor.style.pointerEvents = 'none';
        cursor.style.zIndex = '999999';
        cursor.style.transition = 'transform 0.1s ease, left 0.05s linear, top 0.05s linear';
        cursor.style.boxShadow = '0 0 8px rgba(255, 0, 0, 0.8)';
        document.body.appendChild(cursor);

        // 2. 마우스 이동 이벤트 감지 시 커서 위치 업데이트
        window.addEventListener('mousemove', (e) => {
            cursor.style.left = `${e.clientX - 7}px`;
            cursor.style.top = `${e.clientY - 7}px`;
        });

        // 3. 클릭 시 핑(Ripple) 이펙트 애니메이션 생성
        window.addEventListener('click', (e) => {
            const ripple = document.createElement('div');
            ripple.style.position = 'fixed';
            ripple.style.left = `${e.clientX - 15}px`;
            ripple.style.top = `${e.clientY - 15}px`;
            ripple.style.width = '30px';
            ripple.style.height = '30px';
            ripple.style.border = '3px solid #ff0055';
            ripple.style.borderRadius = '50%';
            ripple.style.pointerEvents = 'none';
            ripple.style.zIndex = '999998';
            ripple.style.animation = 'agent-ripple 0.5s ease-out forwards';
            document.body.appendChild(ripple);

            setTimeout(() => ripple.remove(), 500);
        });

        // 4. Ripple 애니메이션 CSS 주입
        const style = document.createElement('style');
        style.textContent = `
            @keyframes agent-ripple {
                0% { transform: scale(0.5); opacity: 1; }
                100% { transform: scale(2.5); opacity: 0; }
            }
        `;
        document.head.appendChild(style);
    })();
    "#;

    page.evaluate(js_code).await?;
    Ok(())
}

/// 페이지의 document.readyState가 'complete'가 될 때까지 최대 timeout_secs 동안 대기합니다.
pub async fn wait_for_page_load<P: PageTrait>(
    page: &P,
    timeout_secs: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    let wait_js = format!(
        r#"
        new Promise((resolve) => {{
            if (document.readyState === 'complete') {{
                resolve(true);
                return;
            }}

            const timer = setTimeout(() => resolve(false), {});

            window.addEventListener('load', () => {{
                clearTimeout(timer);
                resolve(true);
            }}, {{ once: true }});
        }})
        "#,
        timeout_secs * 1000
    );

    // 1단계: 브라우저 내부 load 이벤트 감지
    let _ = page.evaluate(&wait_js).await;

    // 2단계: SPA 동적 렌더링(Masonry 카드 등) 안정을 위한 추가 폴링 루프
    // 2단계: SPA 동적 렌더링(Masonry 카드 등) 안정을 위한 추가 폴링 루프
    while start_time.elapsed() < timeout {
        let is_complete = match page.evaluate("document.readyState === 'complete'").await {
            Ok(res) => res.trim().trim_matches('"') == "true",
            Err(_) => false,
        };

        if is_complete {
            // SPA DOM 마운트를 위해 짧은 안정화 딜레이 (300ms)
            tokio::time::sleep(Duration::from_millis(300)).await;
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    println!(
        "⚠️ [Page Load Timeout] {}초 내에 페이지 로드가 완료되지 않았으나 계속 진행합니다.",
        timeout_secs
    );
    Ok(())
}
