keywords: html dom 요소 추출, 요약 도구
---

2자연어 요약(텍스트 요약)이 아니라, 웹페이지 전체 HTML 구조에서 **광고, 헤더, 푸터, 사이드바 등 불필요한 노이즈를 제거하고 실제 핵심 본문에 해당하는 DOM Element/div 영역 전체를 원본 HTML 그대로 추출**하는 목적이시군요.

이 경우 "자연어 요약 LLM"을 쓰는 대신, **Visual/DOM 기반 본문 영역 추출(Boilerplate Removal & Main Content DOM Extraction)** 알고리즘 및 관련 오픈소스/도구를 사용하는 것이 정확합니다.

---

## 1. 추천 핵심 DOM/div 추출 오픈소스 & 도구

### ① Readability.js / Mozilla Readability

- **개요:** 파이어폭스(Firefox) 브라우저의 '읽기 전용 모드(Reader View)'를 받치고 있는 모질라 공식 오픈소스입니다.
- **동작 방식:** DOM 트리 요소들의 텍스트 길이, 링크 비율, class/id명(article, content, main 등) 및 태그 밀도를 점수화하여 가장 유력한 **핵심 본문 컨테이너 DOM 노드**를 통째로 추출합니다.
- **장점:**
- 원본 HTML 태그, 링크(`<a>`), 이미지, 내부 CSS class 구조가 그대로 유지됩니다.
- 추출된 DOM을 화면에 그대로 랜더링하면 클릭 가능한 인터랙션(링크 이동 등)이 완벽히 유지됩니다.

- **언어/환경:** JavaScript/Node.js (`@mozilla/readability`), Python 포팅 버전 (`readability-lxml`)

### ② DOM-based Boilerplate Removal (JS / Puppeteer 기반)

- **개요:** 브라우저 랜더링 엔진(Headless Chrome 등)을 통해 실제 레이아웃 위치와 크기(Bounding Box) 및 텍스트 비율을 계산하여 핵심 div를 찾아내는 방식입니다.
- **대표적 접근법:** `window.getComputedStyle()`로 실제 눈에 보이는 텍스트 영역 중 면적이 가장 넓고 단락 밀도가 높은 최상위 컨테이너 `<div>` / `<article>`을 식별.

---

## 2. 사용 가능한 파이프라인 (Node.js 예시)

`JSDOM`과 `@mozilla/readability`를 조합하면, 웹페이지 HTML을 받아 **핵심 본문 div의 원본 HTML 문자열 및 DOM**만 정확히 골라낼 수 있습니다.

```javascript
import { JSDOM } from "jsdom";
import { Readability } from "@mozilla/readability";

function extractMainContentDiv(rawHtml, url) {
  const dom = new JSDOM(rawHtml, { url });
  const reader = new Readability(dom.window.document);
  const article = reader.parse();

  // article.content HTML 문자열 내부에 핵심 본문 div/element 원본(태그, <a> 링크 포함)이 담겨 있습니다.
  return {
    title: article.title,
    contentHtml: article.content, // 원본 구조가 유지된 핵심 본문 HTML (클릭 가능한 링크 포함)
    textContent: article.textContent,
  };
}
```

---

## 3. LLM을 활용한 DOM Node Selector 방식 (HTML이 복잡할 때)

만약 규칙 기반 추출기(Readability 등)가 웹소설, 단일 페이지 앱(SPA), 복잡한 커뮤니티 구조에서 본문 div를 잘못 잡는 경우, **LLM에게 "텍스트 요약" 대신 "본문 div의 CSS Selector"만 답하게 하는 방식**도 많이 쓰입니다.

1. HTML의 구조 트리(DOM tree depth 3~4까지)만 축약해서 LLM에 전달.
2. **프롬프트:** `"이 HTML 구조에서 실제 핵심 본문을 가지고 있는 최상위 div 또는 article의 CSS Selector(예: #main-article, div.content-body) 하나만 출력해."`
3. 추출된 Selector로 원본 DOM에서 해당 `div` 노드를 `querySelector`로 잘라내어 사용.

---

## 핵심 정리

| 방식                       | 특징                                                            | 추천 대상                                |
| -------------------------- | --------------------------------------------------------------- | ---------------------------------------- |
| **Mozilla Readability.js** | 텍스트 밀도/링크 비율 기반으로 핵심 본문 컨테이너 div 원본 추출 | 일반 블로그, 뉴스, 아티클, 게시글 형태   |
| **DOM Selector via LLM**   | LLM에게 CSS Selector만 뽑게 한 뒤 원본 DOM 자르기               | 구조가 불분명하거나 특이한 커뮤니티/웹앱 |

원하시는 환경(Python, Node.js, 브라우저 익스텐션/스크립트 등)을 말씀해 주시면, 클릭 가능한 원본 HTML div 추출 및 랜더링까지 동작하는 실제 예시 코드를 바로 작성해 드릴 수 있습니다.
