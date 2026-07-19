//! Tests for the strict provider-neutral model/tool envelope.

use super::{
    build_reask, build_tool_prompt, parse_tool_call, ExpectedTool, ProtocolErrorCode,
    ProviderFraming, ToolArguments, ToolId, MAX_TOOL_REPLY_BYTES,
};

#[test]
fn accepts_one_exact_summary_submission() {
    let raw = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"A bounded summary."}}"#,
        "</plume_tool_call>",
    );

    let call = parse_tool_call(raw, ExpectedTool::Summary { source_id: "S1" })
        .expect("the exact disclosed summary call should parse");

    assert_eq!(call.call_id, "c1");
    assert_eq!(call.tool, ToolId::ResearchSummarySubmit);
    assert_eq!(
        call.arguments,
        ToolArguments::Summary {
            source_id: "S1".into(),
            summary: "A bounded summary.".into(),
        }
    );
}

#[test]
fn accepts_one_exact_markdown_submission() {
    let raw = concat!(
        "<plume_tool_call>",
        r##"{"callId":"draft-1","tool":"artifact.markdown.submit","arguments":{"markdown":"# Result\n\nClaim [[S1]]"}}"##,
        "</plume_tool_call>",
    );

    let call = parse_tool_call(raw, ExpectedTool::Markdown)
        .expect("the exact disclosed Markdown call should parse");

    assert_eq!(call.call_id, "draft-1");
    assert_eq!(call.tool, ToolId::ArtifactMarkdownSubmit);
    assert_eq!(
        call.arguments,
        ToolArguments::Markdown {
            markdown: "# Result\n\nClaim [[S1]]".into(),
        }
    );
}

#[test]
fn allows_only_whitespace_outside_the_envelope() {
    let raw = concat!(
        " \n<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"Summary."}}"#,
        "</plume_tool_call>\n ",
    );

    assert!(parse_tool_call(raw, ExpectedTool::Summary { source_id: "S1" }).is_ok());
}

#[test]
fn rejects_prose_or_duplicate_envelopes_without_accepting_a_partial_call() {
    let valid = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"Summary."}}"#,
        "</plume_tool_call>",
    );
    for raw in [format!("Here you go: {valid}"), format!("{valid}{valid}")] {
        let error = parse_tool_call(&raw, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("ambiguous framing must fail closed");
        assert_eq!(error.code, ProtocolErrorCode::Envelope);
    }
}

#[test]
fn rejects_invalid_json_unknown_fields_and_unknown_tools() {
    let cases = [
        (
            "<plume_tool_call>{not-json}</plume_tool_call>",
            ProtocolErrorCode::InvalidJson,
        ),
        (
            concat!(
                "<plume_tool_call>",
                r#"{"callId":"c1","tool":"research.summary.submit","extra":true,"arguments":{"sourceId":"S1","summary":"Summary."}}"#,
                "</plume_tool_call>",
            ),
            ProtocolErrorCode::InvalidJson,
        ),
        (
            concat!(
                "<plume_tool_call>",
                r#"{"callId":"c1","tool":"web.search","arguments":{}}"#,
                "</plume_tool_call>",
            ),
            ProtocolErrorCode::UnknownTool,
        ),
    ];

    for (raw, code) in cases {
        let error = parse_tool_call(raw, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("invalid calls must fail closed");
        assert_eq!(error.code, code);
    }
}

#[test]
fn rejects_wrong_phase_source_identity_and_argument_shape() {
    let wrong_phase = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"artifact.markdown.submit","arguments":{"markdown":"Draft [[S1]]"}}"#,
        "</plume_tool_call>",
    );
    assert_eq!(
        parse_tool_call(wrong_phase, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("a Markdown call cannot satisfy a summary turn")
            .code,
        ProtocolErrorCode::WrongPhase,
    );

    let wrong_source = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S2","summary":"Summary."}}"#,
        "</plume_tool_call>",
    );
    assert_eq!(
        parse_tool_call(wrong_source, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("a summary cannot claim another source")
            .code,
        ProtocolErrorCode::Identity,
    );

    let extra_argument = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"Summary.","extra":true}}"#,
        "</plume_tool_call>",
    );
    assert_eq!(
        parse_tool_call(extra_argument, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("unknown arguments must be rejected")
            .code,
        ProtocolErrorCode::InvalidArguments,
    );
}

#[test]
fn rejects_invalid_id_empty_content_and_oversized_reply() {
    let bad_id = concat!(
        "<plume_tool_call>",
        "{\"callId\":\"bad\\ncall\",\"tool\":\"research.summary.submit\",\"arguments\":{\"sourceId\":\"S1\",\"summary\":\"Summary.\"}}",
        "</plume_tool_call>",
    );
    assert_eq!(
        parse_tool_call(bad_id, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("control characters in call ids are refused")
            .code,
        ProtocolErrorCode::Identity,
    );

    let empty = concat!(
        "<plume_tool_call>",
        r#"{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"   "}}"#,
        "</plume_tool_call>",
    );
    assert_eq!(
        parse_tool_call(empty, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("blank summaries are not useful tool results")
            .code,
        ProtocolErrorCode::InvalidArguments,
    );

    let oversized = "x".repeat(MAX_TOOL_REPLY_BYTES + 1);
    assert_eq!(
        parse_tool_call(&oversized, ExpectedTool::Summary { source_id: "S1" })
            .expect_err("oversized replies are refused before parsing")
            .code,
        ProtocolErrorCode::Oversized,
    );
}

#[test]
fn recovery_prompt_is_bounded_and_does_not_echo_model_output() {
    let raw = "SECRET MODEL OUTPUT";
    let error = parse_tool_call(raw, ExpectedTool::Summary { source_id: "S1" })
        .expect_err("plain prose is not a tool call");

    let prompt = build_reask(&error, ExpectedTool::Summary { source_id: "S1" });

    assert!(prompt.len() <= 4096);
    assert!(!prompt.contains(raw));
    assert!(prompt.contains("research.summary.submit"));
    assert!(prompt.contains("S1"));
    assert!(prompt.contains("envelope"));
}

#[test]
fn qwen_and_apple_disclose_the_same_tool_shape() {
    let expected = ExpectedTool::Summary { source_id: "S1" };
    let qwen = build_tool_prompt(ProviderFraming::QwenChatMl, expected);
    let apple = build_tool_prompt(ProviderFraming::AppleInstructions, expected);

    for prompt in [&qwen, &apple] {
        assert!(prompt.instructions.contains("research.summary.submit"));
        assert!(prompt.instructions.contains("<plume_tool_call>"));
        assert!(prompt.instructions.contains("S1"));
        assert!(prompt.instructions.contains("no prose"));
    }
    assert_eq!(qwen.stop_sequence, Some("<|im_end|>"));
    assert_eq!(apple.stop_sequence, None);
}
