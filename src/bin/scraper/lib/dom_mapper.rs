// Cargo.toml
// [dependencies]
// scraper = "0.22"
// ego-tree = "0.6"

use scraper::{Html, Node};
use std::collections::HashMap;

/// HTML 문자열을 파싱하여 각 부모 태그별 직계 자식 태그 목록을 매핑합니다.
pub fn map_dom_structure(html: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let document = Html::parse_document(html);

    // DOM 트리의 모든 노드 순회
    for node_ref in document.tree.nodes() {
        // 현재 노드가 Element(태그)인 경우
        if let Node::Element(element) = node_ref.value() {
            let parent_tag = element.name().to_string();

            // 직계 자식 노드 중 Element 노드만 추출
            let children_tags: Vec<String> = node_ref
                .children()
                .filter_map(|child| {
                    if let Node::Element(child_element) = child.value() {
                        Some(child_element.name().to_string())
                    } else {
                        None // Text, Comment 노드 등은 제외
                    }
                })
                .collect();

            // 자식 요소가 존재하는 경우 HashMap에 추가/확장
            if !children_tags.is_empty() {
                map.entry(parent_tag).or_default().extend(children_tags);
            }
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_dom_structure() {
        let html = r#"
            <!DOCTYPE html>
            <html>
                <head><title>Test</title></head>
                <body>
                    <header>
                        <div class="logo"></div>
                        <nav></nav>
                    </header>
                    <main>
                        <section></section>
                        <aside></aside>
                    </main>
                </body>
            </html>
        "#;

        let dom_map = map_dom_structure(html);

        assert_eq!(
            dom_map.get("header"),
            Some(&vec!["div".to_string(), "nav".to_string()])
        );
        assert_eq!(
            dom_map.get("main"),
            Some(&vec!["section".to_string(), "aside".to_string()])
        );
    }
}
