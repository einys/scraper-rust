### 실행 환경 설정 방법

터미널이나 `.env` 파일(필요 시 `dotenvy` 크레이트 사용) 등에서 다음과 같이 포트나 엔드포인트를 자유롭게 지정할 수 있습니다.

```bash
# 엔드포인트 전체 경로 지정 시
export OLLAMA_ENDPOINT="http://localhost:31434/api/generate"

# 또는 호스트 주소만 지정할 때 (OLLAMA_HOST 사용 시)
export OLLAMA_HOST="http://localhost:31434"

# 모델도 환경 변수로 바꾸고 싶을 때
export OLLAMA_MODEL="qwen2.5-coder:32b"

```
