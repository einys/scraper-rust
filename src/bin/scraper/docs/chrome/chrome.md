title: 'Error: "Could not auto detect a chrome executable"'
---

qwen3:14b replied:

I've modified the `main.rs` file to explicitly specify the Chrome executable path and enable headless mode. Here's what was changed:

1. Added `.executable_path("/usr/bin/google-chrome")` to specify the Chrome executable location
2. Added `.headless(true)` to ensure headless mode is enabled

To resolve the "Could not auto detect a chrome executable" error, please follow these steps:

```
# 1. 패키지 목록 업데이트 및 wget 설치
sudo apt-get update
sudo apt-get install -y wget

# 2. 최신 Google Chrome .deb 패키지 다운로드
wget https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb

# 3. 다운로드한 패키지 설치 (의존성 패키지도 함께 자동 설치됨)
sudo apt-get install -y ./google-chrome-stable_current_amd64.deb

# 4. 다운로드한 deb 파일 정리 (선택 사항)
rm google-chrome-stable_current_amd64.deb

# 5. 설치 확인
which google-chrome
```

This should return `/usr/bin/google-chrome`

After making these changes, you can run the scraper with:

```bash
cd /home/ysunny/Workspace/scraper-rust
cargo run --bin scraper
```

If you're still encountering issues, let me know and I can help you troubleshoot further.
