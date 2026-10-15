use scraper::{Html, Node};

pub fn extract_clean_dom(html_content: &str) -> String {
    let document = Html::parse_document(html_content);

    // 제거할 노이즈 태그 목록 (소문자)
    let noise_tags = [
        "script", "style", "noscript", "svg", "iframe", "header", "footer", "link",
    ];

    let mut texts = Vec::new();

    // DOM 트리의 모든 노드 순회
    for node in document.tree.nodes() {
        // 텍스트 노드(Node::Text)인 경우에만 직접 검사
        if let Node::Text(ref text) = node.value() {
            // 텍스트 노드의 부모 태그(ancestors) 중 노이즈 태그가 포함되어 있는지 확인
            let is_inside_noise = node.ancestors().any(|ancestor| {
                if let Node::Element(ref parent_element) = ancestor.value() {
                    noise_tags.contains(&parent_element.name().to_lowercase().as_str())
                } else {
                    false
                }
            });

            // 노이즈 태그 내부가 아닌 본문 텍스트만 수집
            if !is_inside_noise {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }

        // 이미지 태그(Node::Element)인 경우에만 직접 검사
        if let Node::Element(ref element) = node.value() {
            if element.name() == "img" {
                // 이미지 태그의 부모 태그(ancestors) 중 노이즈 태그가 포함되어 있는지 확인
                let is_inside_noise = node.ancestors().any(|ancestor| {
                    if let Node::Element(ref parent_element) = ancestor.value() {
                        noise_tags.contains(&parent_element.name().to_lowercase().as_str())
                    } else {
                        false
                    }
                });

                // 노이즈 태그 내부가 아닌 이미지 태그만 수집
                if !is_inside_noise {
                    if let Some(src) = element.attr("src") {
                        texts.push(src.to_string());
                    }
                }
            }
        }
    }

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
