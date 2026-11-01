self.browser를 쓰면 크롬이랑 너무 깊게 coupling 이 되어서 추상화를 함

---

### Grafana 로그 전처리 및 포맷 통합

Grafana(Loki/Promtail)에서 내보낸 로그 표기 방식(ISO8601 타임스탬프 접두사 및 Nginx [warn] 라인)으로 인해 기존 파서와 포맷 차이가 발생했던 원인을 규명했습니다.

### SvelteKit 라우팅 로그 동작 차이 원인 규명:

    일반 크롤러(AhrefsBot, Googlebot)는 Hard Navigation(GET /i/[HASH])을 유도하지만,

    직접 패치해 둔 Rust 웹 봇 에이전트는 sanitize_js로 target="_blank" 속성을 제거하고 window.open을 가로채면서 SvelteKit SPA 상태를 유지했기 때문에 Soft Navigation(GET /i/[HASH]/__data.json) 핑만 로그에 남았던 현상을 파악했습니다.

### Rust 크롤러 에이전트 탭 제어 및 아키텍처 개편

- **새 탭 인터셉트 및 Hard Navigation 유도 로직 설계**:
  - DOM을 강제로 변조하지 않고, `target="_blank"` 클릭 시 새로 생긴 탭의 URL을 수집한 뒤 새 탭을 즉시 닫고 기존 탭에서 Direct `goto`를 실행(`navigate_smart`)하도록 설계했습니다.
  - 이로써 원본 사이트 동작 상태를 유지하면서도 Nginx 서버에는 완벽한 Hard Navigation 로그(`GET /i/[HASH]`)가 남도록 구성했습니다.
- **`PageTrait` 탭 제어 추상화 메서드 4종 정의**:
  - `get_tab_list(&self)`: 열린 모든 탭의 URL 목록 반환
  - `close_tab(&self, url)`: 특정 URL을 가진 탭 종료
  - `close_other_tabs(&self)`: 현재 탭을 제외한 타 탭 전체 종료
  - `go_to_tab(&self, url)`: 특정 URL 탭으로 포커스 전환
- **어댑터(Adapter) 패턴의 목적과 구조 정립**:
  - 외부 라이브러리 구체 타입(`chromiumoxide::Page`)과 자체 추상화 인터페이스(`PageTrait`) 사이에 **`ChromiumPageAdapter` 래퍼 구조체**를 두는 디자인 패턴을 확인했습니다.
  - 이를 통해 `self.page.browser()` 방식으로 구체 타입 의존성을 캡슐화하면서, 외부 라이브러리가 변경되어도 `WebAgent` 메인 로직은 상처받지 않도록 아키텍처를 정돈했습니다.
