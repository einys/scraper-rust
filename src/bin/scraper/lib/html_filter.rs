use scraper::{Html, Node};

pub fn extract_clean_dom(html_content: &str) -> String {
    let document = Html::parse_document(html_content);

    // 제거할 태그 목록 (대소문자 구분 없이 비교하기 위해 소문자로 정의)
    let noise_tags = [
        "script", "style", "noscript", "svg", "iframe", "header", "footer", "style", "link",
    ];

    let mut texts = Vec::new();

    // DOM 트리의 노드를 순회
    for node in document.tree.nodes() {
        // 현재 노드가 Element(태그)인 경우
        if let Node::Element(ref element) = node.value() {
            let tag_name = element.name().to_lowercase();

            // 상위 부모 노드 중 노이즈 태그가 하나라도 포함되어 있는지 확인
            let is_inside_noise = node.ancestors().any(|ancestor| {
                if let Node::Element(ref parent_element) = ancestor.value() {
                    noise_tags.contains(&parent_element.name().to_lowercase().as_str())
                } else {
                    false
                }
            });

            // 노이즈 태그 내부가 아닌 경우의 텍스트 노드 처리
            if !is_inside_noise {
                // 직계 자식 텍스트만 수집 (재귀적 수집 방지)
                for child in node.children() {
                    if let Node::Text(ref text) = child.value() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            texts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }

    // 공백으로 이어붙이기 (필요시 \n으로 구분 가능)
    texts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clean_dom() {
        let html = r#"
            <html>
                <body>
                    <style>.bg-red { color: red; }</style>
                    <div class="product">
                        <h1>아이폰 15</h1>
                        <script>console.log("analytics");</script>
                        <span>1,200,000원</span>
                    </div>
                </body>
            </html>
        "#;

        let cleaned = extract_clean_dom(html);
        assert_eq!(cleaned, "아이폰 15 1,200,000원");
    }
}
