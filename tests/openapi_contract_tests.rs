use noyalib::Value;

fn load_openapi() -> Value {
    let raw = std::fs::read_to_string("openapi/openapi.v1.yaml")
        .expect("openapi contract file should exist at openapi/openapi.v1.yaml");
    noyalib::from_str(&raw).expect("openapi contract should be valid YAML")
}

fn lookup<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .unwrap_or_else(|| panic!("missing path segment in openapi contract: {}", segment));
    }
    current
}

#[test]
fn openapi_contract_has_expected_version_and_info() {
    let doc = load_openapi();

    assert_eq!(doc.get("openapi").and_then(Value::as_str), Some("3.1.0"));
    assert_eq!(
        lookup(&doc, &["info", "title"]).as_str(),
        Some("tierpulse API")
    );
    assert_eq!(lookup(&doc, &["info", "version"]).as_str(), Some("1.0.0"));
}

#[test]
fn openapi_contract_contains_required_public_paths() {
    let doc = load_openapi();
    let paths = doc.get("paths").expect("paths should exist");

    assert!(paths.get("/api/v1/analyze").is_some());
    assert!(paths.get("/health/live").is_some());
    assert!(paths.get("/health/ready").is_some());
    assert!(paths.get("/metrics").is_some());
}

#[test]
fn analyze_endpoint_declares_required_responses_and_error_envelope_refs() {
    let doc = load_openapi();

    let responses = lookup(&doc, &["paths", "/api/v1/analyze", "post", "responses"]);
    for status in ["200", "400", "401", "429", "503"] {
        assert!(
            responses.get(status).is_some(),
            "analyze endpoint should define {} response",
            status
        );
    }

    for status in ["400", "401", "429", "503"] {
        let schema_ref = lookup(
            &doc,
            &[
                "paths",
                "/api/v1/analyze",
                "post",
                "responses",
                status,
                "content",
                "application/json",
                "schema",
                "$ref",
            ],
        )
        .as_str();

        assert_eq!(
            schema_ref,
            Some("#/components/schemas/ErrorEnvelope"),
            "status {} should reference ErrorEnvelope",
            status
        );
    }
}

#[test]
fn error_envelope_schema_matches_runtime_contract_fields() {
    let doc = load_openapi();

    let required = lookup(
        &doc,
        &["components", "schemas", "ErrorEnvelope", "required"],
    )
    .as_sequence()
    .expect("ErrorEnvelope.required should be an array");

    let required_fields: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
    for expected in [
        "code",
        "message",
        "retry_after_seconds",
        "request_id",
        "details",
    ] {
        assert!(
            required_fields.contains(&expected),
            "ErrorEnvelope.required should include {}",
            expected
        );
    }
}
