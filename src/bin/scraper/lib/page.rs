use chromiumoxide::Page;
use serde::Deserialize;
use std::error::Error;
use tokio::test;

#[derive(Deserialize, Debug)]
struct ElementRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

async fn get_element_rect(
    page: &Page,
    selector: &str,
) -> Result<Option<ElementRect>, Box<dyn Error>> {
    // JS 문자열 이스케이프 및 안전성을 위해 JSON.stringify 사용
    let selector_json = serde_json::to_string(selector)?;

    let js_script = format!(
        r#"
        (() => {{
            const el = document.querySelector({selector_json});
            if (!el) return null;
            const rect = el.getBoundingClientRect();
            return {{
                x: rect.x + window.scrollX,
                y: rect.y + window.scrollY,
                width: rect.width,
                height: rect.height
            }};
        }})()
        "#
    );

    // null인 경우 deserialization 에러를 피하고 Option<ElementRect>로 매핑
    let result: Option<ElementRect> = page.evaluate(js_script).await?.into_value()?;
    // Option<ElementRect> 반환):
    // serde가 JS의 null을 Rust의 None으로 자연스럽게 변환합니다.
    // 요소를 찾지 못한 상황이 정상적인 흐름(Ok(None))으로 처리되어, match나 if let Some() 문으로 깔끔하게 대응할 수 있습니다.
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use std::error::Error;
    use std::fs::read_to_string;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_get_element_rect() -> Result<(), Box<dyn Error>> {
        // 1. Cargo.toml이 위치한 프로젝트 루트 기준 상대 경로 설정
        let mut test_html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        test_html_path.push("src/bin/scraper/test.html");

        let test_html_content = read_to_string(&test_html_path).map_err(|e| {
            format!(
                "테스트 HTML 파일을 읽을 수 없습니다 ({:?}): {}",
                test_html_path, e
            )
        })?;

        // 2. Headless 브라우저 생성 및 실행
        let (mut browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--headless=new")
                .arg("--no-sandbox")
                .build()?,
        )
        .await?;

        // 3. chromiumoxide 필수: 백그라운드 이벤트 처리 핸들러 태스크 루프
        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });

        let page = browser.new_page("about:blank").await?;

        // 4. HTML 파일 내용을 직접 DOM에 삽입 (navigate 대신 set_content 사용)
        page.set_content(test_html_content).await?;

        // 5. async 함수 내부이므로 block_on 없이 await 직접 사용
        let selector = "#test-element";
        let result = get_element_rect(&page, selector).await?;

        // 요소가 존재하고 Option 내부에 값이 있는지 검증
        assert!(result.is_some(), "#test-element 요소를 찾을 수 없습니다.");

        let rect = result.unwrap();
        println!("추출된 좌표: {:?}", rect);

        // 6. 브라우저 종료 및 핸들러 정리
        browser.close().await?;
        handle.abort();

        Ok(())
    }
}
