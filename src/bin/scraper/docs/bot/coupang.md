keywords: 쿠팡, akamai, 탐지 우회 방법
---

오류 메시지에 포함된 edgesuite.net URL과 Reference #... 지문은 Akamai 보안 차단 페이지에서 발생하는 전형적인 응답 형태입니다.

# 차단 원인 분석

법
Chromiumoxide(CDP 기반) 환경에서 접근할 때 단순 헤더 변경만으로 해결되지 않는 이유는 다음과 같습니다.

    CDP (Chrome DevTools Protocol) 탐지

        CDP를 통해 제어되는 Chrome 인스턴스는 내부적으로 window.navigator.webdriver 외에도 DevTools 커넥션 특유의 JS 런타임 변수와 이벤트 타이밍을 남깁니다.

        Akamai bot detector는 JS 챌린지를 실행하며 CDP 특유의 동작 패턴(오디오/지오메트리 렌더링 피처, 캔버스 핑거프린팅 반응 차이 등)을 실시간으로 스캔합니다.

    TLS / HTTP2 Fingerprinting

        Chrome 실행 시 부여된 커스텀 플래그(--disable-blink-features 등) 조합이나 BoringSSL 사양 차이로 인해 HTTP/2 Fingerprint(JA3/JA4, Client Hello 헤더 순서 등)가 일반 데스크톱 Chrome과 달라져 Edge 단계에서 차단될 수 있습니다.

    IP 평판 (IP Reputation)

        코드가 실행되는 환경(클라우드 인프라, 데이터센터 IP, VPN 등)의 IP 대역이 데이터센터 IP 대역으로 등록되어 있으면 WAF 수준에서 헤더 검사 이전에 즉시 403을 반환합니다.

# Akamai가 Headless Chrome을 잡아내는 원리

    TLS / ClientHello Fingerprinting (JA3 / JA4)

        Chrome 실행 시 부여된 커스텀 명령어 플래그(--disable-gpu, --no-sandbox 등)의 조합으로 인해 Chrome이 서버와 TLS 커넥션을 맺을 때 보내는 Cipher Suites, Extensions 순서가 일반 데스크톱 Chrome과 달라집니다.

        Akamai는 웹 서버로 요청이 전달되기 이전 Edge L7/TLS 레이어에서 이 지문을 확인하고 즉시 403을 반환합니다.

    HTTP/2 SETTINGS & WINDOW_UPDATE 프레임 차이

        일반 브라우저와 자동화 드라이버를 통해 실행된 Chrome은 HTTP/2 초기 악수 과정에서 보내는 SETTINGS 프레임의 헤더 파라미터(STREAM_SIZE, INITIAL_WINDOW_SIZE 등) 순서 및 값이 다릅니다.

    CDP (Chrome DevTools Protocol) 탐지

        CDP로 브라우저를 제어하면 내부적으로 JS 런타임에 탐지 가능한 이벤트 트리거나 마우스 이동/스크롤의 비자연스러운 간격이 수집됩니다.

# 현시점에서의 해결 방안

1. TLS/HTTP2 Fingerprint 우회 클라이언트 사용 (추천)

Rust 백엔드에서 브라우저를 직접 제어하는 대신, TLS/HTTP2 지문을 실제 브라우저와 100% 동일하게 흉내 내는 HTTP 클라이언트를 사용하는 방법입니다.

    rcurl / curl-impersonate 기반 Binding:

        Chrome, Firefox의 TLS ClientHello 지문 및 HTTP/2 프레임을 바이너리 레벨에서 완벽히 모방하는 curl-impersonate 라이브러리를 사용합니다.

        Rust의 reqwest 대신 curl-impersonate C 래퍼를 사용하거나, Python/Node.js의 curl_cffi와 연동하여 HTTP 요청을 보냅니다.

        브라우저를 띄우지 않으므로 리소스 사용량이 매우 적고 Akamai TLS 차단을 우회할 확률이 높습니다.

2. Undetected / Stealth 드라이버 + CDP 포트 연결

순수 chromiumoxide 기본 설정 대신, TLS 패치와 CDP 탐지 회피 기능이 적용된 브라우저 인스턴스를 외부에서 띄우고 Rust에서는 해당 포트에 연결만 하는 구조입니다.

    Undetected-Chromium 바이너리 활용:

        Chrome 바이너리 자체가 패치된 undetected-chromedriver용 Chrome 인스턴스를 원격 디버깅 포트(--remote-debugging-port=9222)로 실행합니다.

        Rust 코드에서는 Browser::connect("[http://127.0.0.1:9222](http://127.0.0.1:9222)") 방식으로 연동합니다.

3. Residential Proxy (주거용 IP) 연동

Akamai는 IP 평판 시스템을 작동하므로, 요청을 보내는 환경이 클라우드/데이터센터 IP 대역이거나 동일 IP에서 짧은 시간 동안 연쇄 요청이 발생할 경우 지문 우회와 상관없이 IP 레벨에서 차단합니다.

    한국 내 통신사(KT/SKT/LGU+) 주거용 IP(Residential Proxy)를 적용해야 WAF 알고리즘의 의심 점수를 낮출 수 있습니다.
