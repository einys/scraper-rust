아니 잠깐, 나는 이 함수가 있잖아. 그럼 그냥 이 return을 iterate하면서 아이템을 하나씩 클릭해도 되는 거잖아?

/// LLM Selector 결과와 브라우저 Page를 받아 위치 및 내용이 통합된 JSON 결과를 반환합니다. 리턴: json형식의 '문자열'.
pub async fn extract_data_with_selectors<P: PageTrait>(
    page: &P,
    schema: &SelectorSchema,
) -> Result<String, DynError> {
    // LLM 결과에서 JSON 부분 추출 (마크다운 `json ... ` 응답 대응)
    // let clean_json_str = parse_json_from_llm_response(llm_selector_json_str);
    // 💡 문자열 파싱 과정(parse_json_from_llm_response)이 제거되고
    //    타입이 보장된 schema 구조체를 직접 활용합니다.
    let mut final_data: HashMap<String, ExtractedElementInfo> = HashMap::new();
    for (field_name, selector) in &schema.fields {
        // 컨테이너 셀렉터와 필드 상대 셀렉터를 결합하거나 단독 셀렉터로 정보 추출
        let full_selector = if schema.container.trim().is_empty() {
            selector.clone()
        } else {
            format!("{} {}", schema.container, selector)
        };

        if let Ok(Some(element_info)) = get_element_info(page, &full_selector).await {
            final_data.insert(field_name.clone(), element_info);
        } else {
            // 요소를 찾지 못했거나 에러인 경우 None 구조 유지
            final_data.insert(
                field_name.clone(),
                ExtractedElementInfo {
                    selector: full_selector,
                    text_content: None,
                    outer_html: None,
                    rect: None,
                },
            );
        }
    }

    let extraction_result = ExtractionResult { data: final_data };

    // 예쁘게 정렬된 JSON 문자열로 반환
    let json_output = serde_json::to_string_pretty(&extraction_result)?;
    Ok(json_output)
}
