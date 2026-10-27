# scraper-rust

주기적으로 지정한 API를 조회하고 응답이 변경되었거나 오류가 발생했을 때, 또는 정기 분석 시간이 되었을 때 로컬 Ollama 모델로 결과를 한국어로 요약하는 모니터링 에이전트입니다. 리포트는 표준 출력에 기록됩니다.

## 실행

Ollama를 실행하고 사용할 모델을 내려받습니다.

```sh
ollama pull llama3.2
```

환경 변수를 설정해 실행합니다. 생략한 값은 아래 기본값을 사용합니다.

```sh
API_URL="http://localhost:8000/api/status" \
OLLAMA_MODEL="llama3.2" \
OLLAMA_HOST="http://localhost:31434"
POLL_INTERVAL_SECS=300 \
ANALYSIS_INTERVAL_SECS=3600 \
cargo run --release
```

| 환경 변수                | 기본값                             | 설명                              |
| ------------------------ | ---------------------------------- | --------------------------------- |
| `API_URL`                | `http://localhost:8000/api/status` | 모니터링할 GET API 주소           |
| `POLL_INTERVAL_SECS`     | `300`                              | API 조회 간격(초)                 |
| `ANALYSIS_INTERVAL_SECS` | `3600`                             | 변경이 없을 때 정기 분석 간격(초) |
| `OLLAMA_URL`             | `http://localhost:11434/api/chat`  | Ollama Chat API 주소              |
| `OLLAMA_MODEL`           | `llama3.2`                         | 로컬에 설치된 Ollama 모델 이름    |
| `REQUEST_TIMEOUT_SECS`   | `10`                               | 대상 API 요청 타임아웃(초)        |

첫 주기는 시작 직후 실행됩니다. API 응답 본문은 프롬프트에 포함할 때 최대 12,000자로 제한합니다. Ollama 요청 타임아웃은 120초이며, 한 주기가 길어져도 밀린 조회를 연속 실행하지 않습니다.

# Test

```
RUST_BACKTRACE=1 cargo test --bin scraper lib::page::tests::test_get_element_rect -- --nocapture
```

```
cargo test
```
