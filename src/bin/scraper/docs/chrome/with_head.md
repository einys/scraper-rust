        .chrome_executable("/usr/bin/google-chrome") // 실행 경로 설정
        .viewport(Viewport {
            width: 1920,
            height: 1080,
            ..Default::default()
        })
        .with_head()
        .arg("--no-sandbox") // 리눅스 서버/도커 환경 필수 플래그
        .arg("--disable-dev-shm-usage") // /dev/shm 메모리 부족 방지
        .arg("--disable-gpu")
        .build()?;
