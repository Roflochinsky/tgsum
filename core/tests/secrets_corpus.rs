use serde::Deserialize;
use tgsum_core::sanitize::{sanitize, ReviewPolicy, RULES_VERSION};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema_version: u8,
    rules_version: String,
    description: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    input: String,
    value: Option<String>,
    rule: Option<String>,
    confidence: Option<String>,
    redacted: Option<String>,
}

#[test]
fn versioned_synthetic_corpus_measures_detection_and_confidence_without_report_leaks() {
    let fixture = include_str!("fixtures/secrets-v2.json");
    let aws_id = regex::Regex::new(r"(?:AKIA|ASIA)[A-Z0-9]{16}").unwrap();
    assert!(
        !aws_id.is_match(fixture),
        "fixture contains a committed AWS-shaped ID"
    );
    let corpus: Corpus = serde_json::from_str(fixture).unwrap();
    assert_eq!(corpus.schema_version, 1);
    assert!(corpus.description.starts_with("Synthetic"));
    let mut ids = std::collections::HashSet::new();
    let mut covered = std::collections::BTreeSet::new();
    let (mut tp, mut fp, mut tn, mut missed, mut high, mut medium) = (0, 0, 0, 0, 0, 0);
    for mut case in corpus.cases {
        let aws_id = match case.id.as_str() {
            "aws-id-candidate" => Some(("__SYNTHETIC_AWS_ID__", "AK")),
            "aws-temporary-id-candidate" => Some(("__SYNTHETIC_AWS_TEMP_ID__", "AS")),
            _ => None,
        };
        if let Some((marker, prefix)) = aws_id {
            // Keep provider-shaped synthetic values in the runtime corpus
            // without committing contiguous token-looking IDs to git.
            assert_eq!(case.input, marker);
            assert_eq!(case.value.as_deref(), Some(marker));
            let synthetic = [prefix, "IA", "TGSUM", "TEST", "0000001"].concat();
            assert_eq!(synthetic.len(), 20);
            case.input = synthetic.clone();
            case.value = Some(synthetic);
        }
        assert!(ids.insert(case.id.clone()), "duplicate corpus ID");
        let result = sanitize(&case.input, ReviewPolicy::KeepForReview).unwrap();
        let detected = !result.report.findings.is_empty();
        match (case.value.is_some(), detected) {
            (true, true) => tp += 1,
            (true, false) => missed += 1,
            (false, true) => fp += 1,
            (false, false) => tn += 1,
        }
        let Some(value) = case.value else {
            assert!(case.rule.is_none() && case.confidence.is_none() && case.redacted.is_none());
            assert!(
                result.text == case.input && !detected,
                "{}: false positive",
                case.id
            );
            continue;
        };
        assert_eq!(
            result.report.findings.len(),
            1,
            "{}: missing/extra finding",
            case.id
        );
        let finding = &result.report.findings[0];
        covered.insert(case.rule.as_ref().unwrap().clone());
        let serialized = serde_json::to_value(finding).unwrap();
        assert_eq!(
            serialized["rule"],
            case.rule.unwrap(),
            "{}: category",
            case.id
        );
        let confidence = case.confidence.unwrap();
        assert_eq!(
            serialized["confidence"], confidence,
            "{}: confidence",
            case.id
        );
        assert_eq!(
            &case.input[finding.input.clone()],
            value,
            "{}: full value",
            case.id
        );
        let expected = case.redacted.unwrap();
        if confidence == "high" {
            high += 1;
            assert_eq!((result.report.redacted, result.report.needs_review), (1, 0));
            assert_eq!(result.text, expected, "{}: automatic redaction", case.id);
        } else {
            assert_eq!(confidence, "medium");
            medium += 1;
            assert_eq!((result.report.redacted, result.report.needs_review), (0, 1));
            assert_eq!(result.text, case.input, "{}: preview", case.id);
        }
        let explicit = sanitize(&case.input, ReviewPolicy::RedactCandidates).unwrap();
        assert_eq!(explicit.text, expected, "{}: reviewed redaction", case.id);
        assert_eq!(explicit.report.needs_review, 0);
        for report in [&result.report, &explicit.report] {
            assert!(
                !serde_json::to_string(report).unwrap().contains(&value),
                "{}: report leak",
                case.id
            );
            assert!(
                !format!("{report:?}").contains(&value),
                "{}: debug leak",
                case.id
            );
        }
        let repeated = sanitize(&explicit.text, ReviewPolicy::RedactCandidates).unwrap();
        assert_eq!(repeated.text, expected, "{}: idempotence", case.id);
        assert!(
            repeated.report.findings.is_empty(),
            "{}: repeated finding",
            case.id
        );
    }
    eprintln!(
        "synthetic corpus: TP={tp}, FP={fp}, TN={tn}, FN={missed}, high={high}, medium={medium}"
    );
    assert_eq!((tp, fp, tn, missed, high, medium), (62, 0, 20, 0, 50, 12));
    assert_eq!(
        covered,
        [
            "private_key",
            "incomplete_private_key",
            "authorization",
            "github_token",
            "provider_token",
            "provider_token_candidate",
            "telegram_bot_token",
            "cookie_value",
            "contextual_entropy",
            "credential_assignment",
            "url_credentials",
            "jwt_candidate",
            "generic_key_assignment"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    assert_eq!(corpus.rules_version, RULES_VERSION);
}
