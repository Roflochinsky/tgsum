use base64::Engine;
use tgsum_core::sanitize::{
    sanitize, Confidence, FindingAction, ReviewPolicy, SecretRule, MAX_FIELD_BYTES, MAX_FINDINGS,
    REPLACEMENT, RULES_VERSION,
};

#[test]
fn explicit_credentials_are_removed_without_leaking_into_reports() {
    // These are deliberate synthetic strings, never account credentials.
    let cases = [
        (
            "Authorization: Bearer SYNTHETIC-value._~+/==",
            "SYNTHETIC-value._~+/==",
            SecretRule::Authorization,
        ),
        (
            r#"{"Authorization":"bAsIc c3ludGhldGljOnBhc3N3b3Jk"}"#,
            "c3ludGhldGljOnBhc3N3b3Jk",
            SecretRule::Authorization,
        ),
        (
            "proxy-authorization: bearer SYNTHETIC",
            "SYNTHETIC",
            SecretRule::Authorization,
        ),
        (
            "ghp_SYNTHETIC_NOT_A_REAL_TOKEN_0000000000",
            "ghp_SYNTHETIC_NOT_A_REAL_TOKEN_0000000000",
            SecretRule::GithubToken,
        ),
        (
            "github_pat_SYNTHETIC_NOT_A_REAL_TOKEN_0000",
            "github_pat_SYNTHETIC_NOT_A_REAL_TOKEN_0000",
            SecretRule::GithubToken,
        ),
        (
            "DB_PASSWORD='synthetic with spaces'",
            "synthetic with spaces",
            SecretRule::CredentialAssignment,
        ),
        (
            r#"{"password":"synthetic\"rest-of-secret"}"#,
            r#"synthetic\"rest-of-secret"#,
            SecretRule::CredentialAssignment,
        ),
        (
            "api_key=SYNTHETIC&next=keep",
            "SYNTHETIC",
            SecretRule::CredentialAssignment,
        ),
        (
            "token=legacy-without-provider-prefix",
            "legacy-without-provider-prefix",
            SecretRule::CredentialAssignment,
        ),
        (
            "PASSWORD='$starts-with-dollar'",
            "$starts-with-dollar",
            SecretRule::CredentialAssignment,
        ),
        (
            "PASSWORD=\"SYNTHETIC unfinished value\nnext line",
            "SYNTHETIC unfinished value",
            SecretRule::CredentialAssignment,
        ),
        ("PASSWORD='$'", "$", SecretRule::CredentialAssignment),
        (
            "AWS_SECRET_KEY=SYNTHETIC",
            "SYNTHETIC",
            SecretRule::CredentialAssignment,
        ),
        (
            "postgres://test:synthetic%40pass@localhost:5432/db",
            "test:synthetic%40pass",
            SecretRule::UrlCredentials,
        ),
        (
            "https://user:synthetic@[::1]:80/path",
            "user:synthetic",
            SecretRule::UrlCredentials,
        ),
    ];
    for (input, secret, rule) in cases {
        let result = sanitize(input, ReviewPolicy::KeepForReview).unwrap();
        assert!(
            !result.text.contains(secret),
            "rule {rule:?} left a credential"
        );
        assert_eq!(result.report.rules_version, RULES_VERSION);
        assert_eq!(result.report.redacted, 1);
        assert_eq!(result.report.needs_review, 0);
        assert_eq!(result.report.findings[0].rule, rule);
        assert_eq!(result.report.findings[0].confidence, Confidence::High);
        let report = serde_json::to_string(&result.report).unwrap();
        assert!(!report.contains(secret));
        assert!(!format!("{:?}", result.report).contains(secret));
        assert_eq!(
            &result.text[result.report.findings[0].output.clone()],
            REPLACEMENT
        );
    }
}

#[test]
fn variable_length_github_installation_tokens_are_not_truncated() {
    let token = format!(
        "ghs_123_{}.{}.{}",
        "A".repeat(220),
        "B".repeat(220),
        "C".repeat(80)
    );
    let input = format!("before `{token}` after");
    let result = sanitize(&input, ReviewPolicy::KeepForReview).unwrap();
    assert_eq!(result.text, format!("before `{REPLACEMENT}` after"));
    assert_eq!(result.report.redacted, 1);
}

#[test]
fn short_authorization_credentials_do_not_consume_the_next_header() {
    let result = sanitize(
        "Authorization: Bearer x\r\nHost: example.test",
        ReviewPolicy::KeepForReview,
    )
    .unwrap();
    assert_eq!(
        result.text,
        format!("Authorization: Bearer {REPLACEMENT}\r\nHost: example.test")
    );
    assert_eq!(result.report.redacted, 1);
}

#[test]
fn private_key_blocks_and_unfinished_keys_hide_the_whole_payload() {
    for label in [
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "RSA PRIVATE KEY",
        "DSA PRIVATE KEY",
        "EC PRIVATE KEY",
    ] {
        let input = format!(
            "prefix\n-----BEGIN {label}-----\nSYNTHETIC\nONLY\n-----END {label}-----\nsuffix"
        );
        let result = sanitize(&input, ReviewPolicy::KeepForReview).unwrap();
        assert_eq!(result.text, format!("prefix\n{REPLACEMENT}\nsuffix"));
        assert_eq!(result.report.redacted, 1);
        let unfinished = format!("prefix\n-----BEGIN {label}-----\nSYNTHETIC_UNFINISHED");
        let result = sanitize(&unfinished, ReviewPolicy::KeepForReview).unwrap();
        assert_eq!(result.text, format!("prefix\n{REPLACEMENT}"));
        assert_eq!(
            result.report.findings[0].rule,
            SecretRule::IncompletePrivateKey
        );
    }
    let nested = "-----BEGIN PRIVATE KEY-----\n-----BEGIN RSA PRIVATE KEY-----\npayload";
    assert_eq!(
        sanitize(nested, ReviewPolicy::KeepForReview).unwrap().text,
        REPLACEMENT
    );
}

fn encoded(value: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
}

#[test]
fn uncertain_values_require_review_or_explicit_redaction() {
    let jws = format!(
        "{}.{}.synthetic",
        encoded(r#"{"alg":"HS256","typ":"JWT"}"#),
        encoded(r#"{"test":true}"#)
    );
    let unsecured = format!(
        "{}.{}.",
        encoded(r#"{"alg":"none"}"#),
        encoded(r#"{"test":true}"#)
    );
    let jwe = format!(
        "{}.key.iv.cipher.tag",
        encoded(r#"{"alg":"dir","enc":"A256GCM"}"#)
    );
    for token in [&jws, &unsecured, &jwe] {
        let input = format!("Синтетический пример: {token}");
        let review = sanitize(&input, ReviewPolicy::KeepForReview).unwrap();
        assert_eq!(review.text, input);
        assert_eq!(review.report.redacted, 0);
        assert_eq!(review.report.needs_review, 1);
        assert_eq!(review.report.findings[0].rule, SecretRule::JwtCandidate);
        assert_eq!(review.report.findings[0].action, FindingAction::NeedsReview);
        let redacted = sanitize(&input, ReviewPolicy::RedactCandidates).unwrap();
        assert_eq!(
            redacted.text,
            format!("Синтетический пример: {REPLACEMENT}")
        );
        assert_eq!(redacted.report.needs_review, 0);
    }
    // A header supplies stronger context, so the same JWT is hidden automatically.
    let result = sanitize(
        &format!("Authorization: Bearer {jws}"),
        ReviewPolicy::KeepForReview,
    )
    .unwrap();
    assert_eq!(result.report.redacted, 1);
    assert_eq!(result.report.needs_review, 0);
    assert_eq!(result.text, format!("Authorization: Bearer {REPLACEMENT}"));

    let review = sanitize("key=ordinary-dictionary-key", ReviewPolicy::KeepForReview).unwrap();
    assert_eq!(review.report.needs_review, 1);
    assert_eq!(review.text, "key=ordinary-dictionary-key");
}

#[test]
fn overlap_utf8_offsets_and_second_pass_are_consistent() {
    let input =
        "🔒 пароль: PASSWORD='ghp_SYNTHETIC_NOT_A_REAL_TOKEN_0000000000' и TOKEN='секрет🔑'";
    let result = sanitize(input, ReviewPolicy::KeepForReview).unwrap();
    assert_eq!(
        result.text,
        format!("🔒 пароль: PASSWORD='{REPLACEMENT}' и TOKEN='{REPLACEMENT}'")
    );
    assert_eq!(result.report.redacted, 2);
    for finding in &result.report.findings {
        assert!(
            input.is_char_boundary(finding.input.start)
                && input.is_char_boundary(finding.input.end)
        );
        assert_eq!(&result.text[finding.output.clone()], REPLACEMENT);
    }
    let again = sanitize(&result.text, ReviewPolicy::RedactCandidates).unwrap();
    assert_eq!(again.text, result.text);
    assert_eq!(again.report.redacted, 0);
    assert_eq!(again.report.needs_review, 0);
}

#[test]
fn normal_prose_public_keys_urls_and_placeholders_remain_unchanged() {
    for text in [
        "Version v1.2.3 and example.com do not become JWTs.",
        "ghp_example is a prefix example, not a full token.",
        "Authorization tells the server how to authenticate.",
        "https://example.test/path/user:password@file",
        "https://example.test/?next=user:password@file",
        "test@example.test and https://user@example.test/",
        "-----BEGIN PUBLIC KEY-----\nSYNTHETIC\n-----END PUBLIC KEY-----",
        "PASSWORD=${PASSWORD_FROM_ENV}",
        "token='<your-token>'",
        "secret=null",
        "ordinary words: secret, password, token",
    ] {
        let result = sanitize(text, ReviewPolicy::KeepForReview).unwrap();
        assert_eq!(result.text, text);
        assert!(result.report.findings.is_empty());
    }
}

#[test]
fn oversized_fields_fail_without_echoing_content() {
    let input = "X".repeat(MAX_FIELD_BYTES + 1);
    let error = match sanitize(&input, ReviewPolicy::KeepForReview) {
        Err(error) => error,
        Ok(_) => panic!("an oversized field must not be partially scanned"),
    };
    assert!(error.to_string().contains("16 MiB"));
    assert!(!error.to_string().contains('X'));
}

#[test]
fn too_many_findings_fail_instead_of_returning_partially_sanitized_text() {
    let input = "token=SYNTHETIC\n".repeat(MAX_FINDINGS + 1);
    let error = match sanitize(&input, ReviewPolicy::KeepForReview) {
        Err(error) => error,
        Ok(_) => panic!("findings budget must not truncate the scan"),
    };
    assert!(error.to_string().contains("findings limit"));
    assert!(!error.to_string().contains("SYNTHETIC"));
}
