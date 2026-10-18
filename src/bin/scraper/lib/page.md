왜 Vec<HashMap<String, String>> 형태로 추출해?

제미나이

`target_description`에 따라 **어떤 데이터 필드가 나올지 미리 알 수 없는 가변적인 구조**이기 때문입니다.

Rust는 엄격한 정적 타입 언어이므로 컴파일 시점에 구조체의 필드명을 미리 확정해야 합니다. 하지만 LLM이 뽑아낼 결과물은 요청에 따라 계속 달라집니다.

---

### 1. `HashMap<String, String>` (단일 항목 데이터)

LLM이 추출할 필드 이름(`"text"`, `"username"`, `"href"` 등)과 그 필드의 이름을 컴파일 타임에 고정할 수 없으므로, **키-값(Key-Value) 쌍을 동적으로 저장**하기 위해 `HashMap`을 사용합니다.

- 예를 들어, 이번 요청에서는 다음과 같이 한 항목이 들어갑니다:

```json
{
  "username": "charlotte",
  "text": "커미션 열었습니다!",
  "href": "/post/123"
}
```

---

### 2. `Vec<...>` (목록/배열 데이터)

페이지 안에는 위와 같은 게시글 카드가 **여러 개(0개 이상)** 존재합니다. 따라서 동적 크기를 갖는 목록을 표현하기 위해 `Vec`으로 감싸게 됩니다.

---

### 💡 만약 다른 형태로 다루고 싶다면?

1. **`serde_json::Value` 형태:**
   `HashMap` 대신 `serde_json::Value` (주로 `Value::Array` 안에 `Value::Object`) 형태로 반환하도록 바꾸면 나중에 바로 `.to_string()`을 통해 JSON 문자열로 직렬화하기가 더 용이합니다.
2. **구체적인 Struct 사용:**
   만약 수집하려는 데이터 스키마가 항상 고정되어 있다면(예: 언제나 `text`, `username`, `href`만 추출함), 아래처럼 명시적인 Rust 구조체를 정의해서 파싱하는 것이 타입 안정성 면에서 훨씬 좋습니다:

```rust
pub struct PostItem {
    pub text: String,
    pub username: String,
    pub href: String,
    pub img_src: Option<String>,
    pub link: Option<String>,
}

pub fn parse_items(...) -> Result<Vec<PostItem>, DynError>

```
