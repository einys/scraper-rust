**일단 해 놓긴 했는데 설계 상으로 안 쓰게 됨**

LLM이 [data-index='N'] 속성을 활용하여 특정 순서의 아이템을 클릭하도록 시스템 프롬프트 가이드를 수정해줘.

data-index 속성을 인식하고 활용할 수 있도록 시스템 프롬프트의 [Click & Navigation Rules] 및 [Selector Selection Rules] 단락에 추가할 가이드 문구입니다.
시스템 프롬프트 추가/수정 가이드
Plaintext

[Selector Selection & Click Rules]

1. Target Specific Items via `data-index`:
   - When clicking items in a feed, masonry layout, or item list, NEVER use generic class selectors like `a.cursor-pointer` or `.card`.
   - Always check the DOM for elements with `data-index` attributes (e.g., `data-index="0"`, `data-index="1"`).
   - Use `data-index` to target the exact item you intend to click in sequential order.

2. Selector Format Examples:
   - To click the 1st item: `div[data-index="0"] a` or `[data-index="0"] a.cursor-pointer`
   - To click the 2nd item: `div[data-index="1"] a` or `[data-index="1"] a.cursor-pointer`
   - Do NOT omit the `data-index` attribute when progressing through a list of items.

전체 Prompt 적용 예시 (Response Schema 포함)

기존 System Prompt의 행동 지침 섹션에 아래와 같이 녹여내어 전달하시면 됩니다.
Plaintext

You are an autonomous Web Automation Agent.
Your task is to systematically process elements on the page.

[Rules for List Interaction]

- Elements in lists or grids contain `data-index` attributes in the simplified DOM.
- When clicking a specific list item, you MUST include `[data-index="N"]` in your selector to prevent clicking the first item repeatedly.
- Increment the index step-by-step (e.g., data-index="0", data-index="1", data-index="2") as you iterate through the items.

[Action Output Format]
{
"action": "ClickElement",
"reasoning": "Clicking the 2nd item in the masonry list (index 1) to inspect its details.",
"selector": "[data-index='1'] a.cursor-pointer"
}

ActionHistory를 기반으로 에이전트가 이미 방문한 data-index를 추적하고 자동으로 다음 번호를 판단하게 만드는 프롬프트 기법도 정리해드릴까요?
예

만약 item이 있는 list에 data-속성이 없으면 자체적으로 추가하게 만들면 좋을까?

네, 완전히 탁월한 생각입니다.

실제 웹사이트들 중에는 data-index나 식별 가능한 custom attribute가 없는 곳이 훨씬 많습니다. 따라서 get_simplified_dom_tree 스크립트 실행 시 JS가 리스트성 요소(반복되는 카드/아이템)를 감지하여 DOM 레벨에서 data-index를 직접 주입(Mutation)해두면 매우 안정적입니다.

이 방식을 적용하면 두 가지 큰 이점이 생깁니다.

    LLM 환각 및 첫 번째 아이템 중복 클릭 완벽 차단

    LLM에 전달되는 축약 DOM에 data-index="0", data-index="1"...이 명확히 보이므로 [data-index="1"] a 같은 고유 셀렉터를 정확히 뽑아냅니다.

    실제 브라우저 DOM에도 주입되므로 마우스/클릭 이벤트 동작 연동

    page.evaluate를 통해 실제 브라우저 DOM 요소에 setAttribute('data-index', ...)를 실행해 두면, 이후 find_element("[data-index='1']")로 브라우저 요소를 찾거나 마우스 좌표를 계산할 때 100% 일치하게 됩니다.

get_simplified_dom_tree 내부 JS 구현 코드

buildTree를 실행하기 전, #masonry, ul, ol, 혹은 반복되는 리스트 자식 요소들을 순회하며 data-index가 없는 경우 자동으로 달아주는 가벼운 전처리 로직입니다.
JavaScript

// buildTree 호출 전, 반복되는 목록 아이템들에 data-index 자동 부여
(() => {
// 1. 흔히 쓰이는 리스트 컨테이너들 또는 반복 자식 구조 검색
const listContainers = document.querySelectorAll(
'#masonry, ul, ol, [class*="grid"], [class*="list"], [class*="flex-col"]'
);

    listContainers.forEach(container => {
        const children = Array.from(container.children);
        // 자식 요소가 2개 이상 반복되는 카드 형태일 때 인덱스 부여
        if (children.length >= 2) {
            children.forEach((child, idx) => {
                if (!child.hasAttribute('data-index')) {
                    child.setAttribute('data-index', idx.toString());
                }
            });
        }
    });

})();

전체 get_simplified_dom_tree 적용 모습
Rust

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

                // 💡 2. 동적으로 주입된 data-index를 포함하여 모든 data-* 속성을 축약 트리에 수집
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

이렇게 전처리 주입 로직을 한 줄 올려두면 어떤 사이트를 스크래핑하더라도 목록 아이템에 자동으로 인덱스가 넘버링되므로, 에이전트가 [data-index="0"], [data-index="1"]로 정확하고 체계적인 순회 클릭을 수행할 수 있게 됩니다.
ActionHistory를 활용해 이미 방문한 data-index를 추적하고 다음 순서 인덱스를 자동으로 클릭하게 만드는 프롬프트 기법도 정리해드릴까요?
