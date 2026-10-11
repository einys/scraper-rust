# Ollama

kubernetes에 올리고 도메인이랑 연결하고
ollama 서비스를 nodeport로 노출시켜서 로걸에서도 31434 포트로 접근 가능해짐

~/.zshrc에
export OLLAMA_HOST="http://localhost:31434"

gpu가속 적용된 상태인지 확인
k logs -n llm deployment/ollama | grep -i "gpu\|cuda\|cpu"
nvidia-smi

gpu 가속 설정
microk8s enable gpu

# 현재 로드된 모델 및 자원 점유 확인

k exec -it -n llm deployment/ollama -- ollama ps

# ollama 강제 재부팅 (k8s)

# Pod 개수를 0으로 줄여 완전히 프로세스 종료

k scale deployment/ollama -n llm --replicas=0

# 5초 대기 후 다시 1개로 복구

k scale deployment/ollama -n llm --replicas=1

# 새로 뜬 Pod 상태 확인

k get pods -n llm
