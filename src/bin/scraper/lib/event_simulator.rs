// 수정 코드
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::Page; // Page 타입 추가
use futures::StreamExt; // Handler 루프 처리를 위해 필요한 트레이트

pub async fn interact_with_element(page: &Page) -> Result<(), Box<dyn std::error::Error>> {
    // 요소 찾기
    let input_element = page.find_element("input#search").await?;

    // 요소 클릭 및 입력
    input_element.click().await?;
    input_element.type_str("search_term").await?;

    Ok(())
}
