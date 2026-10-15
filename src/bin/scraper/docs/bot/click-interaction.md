keywords: 좌표 알아내기, DOM 트리 축약
---

- ① DOM 트리 축약 (Tree Pruning) — _가장 추천_

원문 HTML을 그대로 LLM에 넘기지 않고, 태그 이름, class/id, 그리고 텍스트 길이 정도만 남긴 "골격 트리(Skeleton Tree)"를 만듭니다.

---

## 1. LLM 기반 CSS Selector 방식이 과연 맞을까?

결론부터 말씀드리면, **"x, y 좌표와 클릭 가능성(Layout)"이 필요한 목적이라면 pure HTML 텍스트만 LLM에 넣는 방식은 한계가 있고 비용이 많이 듭니다.**

### 이유 1: HTML 텍스트만으로는 x, y 좌표를 알 수 없음

HTML 소스 코드(DOM 텍스트)에는 픽셀 단위의 x, y 위치나 실제 클릭 가능 여부(Overlay 가림, `display: none`, CSS `position: absolute` 등) 정보가 없습니다. 좌표와 실제 화면 배치를 얻으려면 **headless 브라우저(Headless Chrome / Playwright 등)로 페이지를 랜더링(Layout/Reflow)해야만** 합니다.

### 이유 2: 복잡한 HTML과 로컬 LLM의 토큰 문제

전체 DOM을 분할해서 여러 번 LLM에 넣는 방식(Chunking)은 **DOM의 부모-자식 트리 문맥이 깨지기 쉽습니다.** 또한, 여러 번 호출 시 추론 시간이 크게 늘어납니다.

---

## 2. 권장하는 Rust 기반 하이브리드 접근법

가장 빠르고 정확하며 토큰 문제도 해결되는 아키텍처는 [1단계: Headless 브라우저로 DOM 구조 축약 및 1차 필터링] ➔ [2단계: (필요시만) 경량 LLM으로 Selector 결정]입니다.

```text
[웹페이지 랜더링 (chromiumoxide / headless_chrome)]
       │
       ▼
[1차 DOM 축약 및 트리 단순화 (Rust)]
  - 렌더링되지 않는 노드(display:none, opacity:0 등) 즉시 제거
  - script, style, svg, meta 등 제거
  - 각 노드의 x, y, width, height, is_clickable 정보 추출
       │
       ▼
[2차 핵심 영역 식별 (알고리즘 or 로컬 LLM)]
  - 텍스트 밀도 + 화면 중앙 근처 대형 div 선택 (알고리즘)
  - 또는 축약된 간소화 트리만 로컬 LLM에 1회 전달해 Selector 추출
       │
       ▼
[최종 결과: 선택된 div의 HTML 원본 + x, y, width, height 좌표]

```

---

## 3. 대형 HTML을 여러 번 나누어 처리하는 기법 (Tree Trimming & Chunking)

토큰 제한을 극복하면서 DOM 트리를 안전하게 줄이는 기법들입니다.

### ① DOM 트리 축약 (Tree Pruning) — _가장 추천_

원문 HTML을 그대로 LLM에 넘기지 않고, 태그 이름, class/id, 그리고 텍스트 길이 정도만 남긴 "골격 트리(Skeleton Tree)"를 만듭니다.

```html
<!-- 원본 (몇십 KB) -->
<div class="article-wrapper" style="...">
  <div class="author-info"><span>Writer: John</span></div>
  <p class="content-text">주요 본문 내용 아주 긴 텍스트...</p>
</div>

<!-- Rust에서 축약한 Skeleton (몇백 바이트) -->
<div id="root">
  <div class="article-wrapper" path="1">
    <div class="author-info" path="1.1">text_len: 17</div>
    <p class="content-text" path="1.2">text_len: 1500</p>
  </div>
</div>
```

이렇게 변환하면 아무리 큰 페이지도 **1~2k 토큰 이내로 축약**되어 로컬 LLM에 한 번에 들어갑니다. LLM에게는 `path="1"`이나 CSS Selector만 반환하게 하면 됩니다.

### ② 계층적 하향식 탐색 (Top-Down Hierarchical Chunking)

만약 트리가 그래도 크다면, **자식 노드 세부사항을 숨기고 깊이(Depth)별로 나누어 요청**합니다.

1. **Pass 1 (루트의 직계 자식들만 탐색):**
   `<body>` 아래의 1차 레벨 `<div>`, `<header>`, `<main>`, `<aside>`의 class/id 및 포함된 총 텍스트 길이만 LLM에 전달.
   ➔ LLM 답변: `"main.container 영역에 본문이 있습니다."`
2. **Pass 2 (선택된 영역의 하위 탐색):**
   `main.container` 내부의 자식 div들만 다시 LLM에 전달.
   ➔ LLM 답변: `"div.article-body 가 실제 본문 div입니다."`

이 방식은 대형 DOM이라도 **2~3번의 가벼운 LLM 호출**로 분할 처리할 수 있습니다.

---

## 4. Rust 생태계 구현 라이브러리 추천

Rust에서 이를 구현할 때 추천하는 크레이트(Crate) 구성입니다.

| 역할                             | 추천 Crate                             | 특징                                                                                                        |
| -------------------------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **Headless Browser & 좌표 측정** | `chromiumoxide` 또는 `headless_chrome` | 실제 Chrome을 띄워 페이지 랜더링 후 `getBoundingClientRect()`로 **x, y, width, height 좌표** 직접 수집 가능 |
| **DOM 파싱 및 HTML 조작**        | `scraper` 또는 `kuchikiki`             | HTML 원본에서 특정 Selector의 DOM 노드 추출 및 트리 순회/축약                                               |
| **로컬 LLM 연동**                | `ollama-rs` 또는 `llm`                 | Ollama 등에 구축된 로컬 LLM(Qwen2.5 등)에 축약된 DOM 프롬프트 전달                                          |

### 좌표 수집 Rust 예시 개념 (chromiumoxide 활용)

```rust
// headless 브라우저에서 특정 selector의 x, y 좌표 및 크기 추출
let element = page.find_element("div.content-body").await?;
let bounding_box = element.get_box_model().await?;

println!("x: {}, y: {}", bounding_box.content[0].x, bounding_box.content[0].y);

```

---

## 요약 및 제안

1. **x, y 좌표 및 클릭 인터랙션**이 목적이라면, **`chromiumoxide` 같은 Headless 브라우저 사용이 필수**입니다.
2. **토큰 문제**는 HTML 전체를 넣지 않고 **Rust에서 DOM을 '골격(Skeleton Tree)' 형태로 대폭 축약**하거나, **Top-Down으로 깊이별 분할 탐색**을 진행하면 완벽히 해결됩니다.
