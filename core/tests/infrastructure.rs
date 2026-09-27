use std::collections::BTreeSet;
use tgsum_core::infrastructure::{
    InfrastructureCategory as Kind, InfrastructureDetector, InfrastructurePolicy,
};
use tgsum_core::project::ProjectStore;

fn policy(categories: &[Kind]) -> InfrastructurePolicy {
    InfrastructurePolicy {
        categories: BTreeSet::from_iter(categories.iter().copied()),
        ..Default::default()
    }
}

fn transform(text: &str, policy: InfrastructurePolicy) -> (String, usize) {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Synthetic infra").unwrap();
    let detector = InfrastructureDetector::new(policy).unwrap();
    let scan = detector.scan(text).unwrap();
    let assigned = store
        .assign_pseudonyms(&project.project_id, 0, &scan.inputs())
        .unwrap();
    let Some(reference) = assigned.project.pseudonyms else {
        return (text.to_owned(), 0);
    };
    let mapping = store
        .load_pseudonyms(&project.project_id, &reference)
        .unwrap();
    let output = scan.apply(&mapping).unwrap();
    (output.text, output.findings.len())
}

#[test]
fn parsed_addresses_keep_identity_and_public_urls_are_untouched() {
    let original = "IP 192.0.2.1:443 [2001:0DB8:0:0:0:0:0:1]:22 2001:db8::1; zone [fe80::1%eth0]. https://EXAMPLE.com:443/a/../b/192.0.2.1?q=/home/alice";
    let (text, count) = transform(original, policy(&[Kind::Ip]));
    assert_eq!(count, 4);
    assert_eq!(text, "IP IP_0001:443 [IP_0002]:22 IP_0002; zone [IP_0003]. https://EXAMPLE.com:443/a/../b/192.0.2.1?q=/home/alice");
    for unchanged in [
        "v1.2.3.4.5 192.168.001.2 999.2.3.4",
        "a192.0.2.1 192.0.2.1.example",
        "host::badword",
        "[fe80::1%]",
        "https://[fe80::1%25eth0]/path",
    ] {
        assert_eq!(transform(unchanged, policy(&[Kind::Ip])).0, unchanged);
    }
}

#[test]
fn internal_urls_use_parsed_identity_and_independent_url_policy() {
    let original = "https://DB.INTERNAL:443/a?x=1&y=2 https://db.internal/a?x=1&y=2 https://db.internal/A?x=1&y=2 https://db.internal/a?y=2&x=1 https://example.com/home/alice http://10.0.0.1/log http://localhost:3000/";
    let (text, count) = transform(original, policy(&[Kind::Url]));
    assert_eq!(count, 6);
    let parts: Vec<_> = text.split_whitespace().collect();
    assert_eq!(parts[0], parts[1]);
    assert_ne!(parts[1], parts[2]);
    assert_ne!(parts[1], parts[3]);
    assert_eq!(parts[4], "https://example.com/home/alice");
    assert!(
        parts[0].starts_with("URL_")
            && parts[5].starts_with("URL_")
            && parts[6].starts_with("URL_")
    );
    assert_eq!(
        transform(original, policy(&[Kind::Ip, Kind::Host])).0,
        original
    );
}

#[test]
fn explicit_names_dns_boundaries_and_contextual_usernames_are_independent() {
    let mut selected = policy(&[Kind::Host, Kind::Domain, Kind::Username]);
    selected.internal_domains = vec!["CORP.EXAMPLE.".into()];
    selected.hostnames = vec!["prod-db-03".into()];
    let input = "host=PROD-db-03 prod-db-03 api.corp.example API.CORP.EXAMPLE corp.example api.corp.example.evil localhost app.local app.home.arpa ssh Dev@db.local username=Dev contact Dev@db.local public.example internal local";
    let (output, count) = transform(input, selected.clone());
    assert_eq!(count, 11);
    let parts: Vec<_> = output.split_whitespace().collect();
    assert_eq!(parts[0].strip_prefix("host=").unwrap(), parts[1]);
    assert_eq!(parts[2], parts[3]);
    assert!(parts[4].starts_with("DOMAIN_"));
    assert_eq!(parts[5], "api.corp.example.evil");
    let (user, host) = parts[10].split_once('@').unwrap();
    assert!(user.starts_with("USER_") && host.starts_with("HOST_"));
    assert_ne!(parts[11].strip_prefix("username=").unwrap(), user);
    assert!(output.ends_with("contact Dev@db.local public.example internal local"));
    selected.categories = BTreeSet::from([Kind::Domain]);
    let only_domain = transform(
        "corp.example api.corp.example host=prod-db-03 username=Dev",
        selected,
    )
    .0;
    assert_eq!(
        only_domain,
        "DOMAIN_0001 api.corp.example host=prod-db-03 username=Dev"
    );
}

#[test]
fn username_identity_respects_known_host_scope() {
    let (text, _) = transform(
        "ssh deploy@a.local ssh deploy@A.LOCAL ssh deploy@b.local username=deploy",
        policy(&[Kind::Username]),
    );
    let words: Vec<_> = text.split_whitespace().collect();
    let a = words[1].split('@').next().unwrap();
    assert_eq!(a, words[3].split('@').next().unwrap());
    assert_ne!(a, words[5].split('@').next().unwrap());
    assert_ne!(a, words[6].strip_prefix("username=").unwrap());
}

#[test]
fn lexical_paths_include_quotes_and_unc_without_touching_routes_or_url_paths() {
    let input = r#"/home/alice/app.log "/home/alice/My Notes.txt" C:\Users\Alice\a.txt c:/Users/Alice/a.txt \\FILESRV\Share\a.log \\filesrv\Share\a.log https://example.com/home/alice /api/v1 path=/custom/project/file.txt relative/a.txt C:relative.txt "\\?\C:\Users\Alice\a.txt""#;
    let (text, count) = transform(input, policy(&[Kind::Path, Kind::Host, Kind::Ip]));
    assert_eq!(count, 7);
    let words: Vec<_> = text.split_whitespace().collect();
    assert!(words[0].starts_with("PATH_") && words[1].starts_with("\"PATH_"));
    assert_eq!(words[2], words[3]);
    assert_eq!(words[4], words[5]);
    assert_eq!(words[6], "https://example.com/home/alice");
    assert_eq!(words[7], "/api/v1");
    assert!(words[8].starts_with("path=PATH_"));
    assert_eq!(words[9], "relative/a.txt");
    assert_eq!(words[10], "C:relative.txt");
    assert_eq!(words[11], r#""\\?\C:\Users\Alice\a.txt""#);
    let protected = "/home/192.0.2.1/app.log https://example.com/192.0.2.1";
    assert_eq!(transform(protected, policy(&[Kind::Ip])).0, protected);
}

#[test]
fn cloud_ids_are_whole_values_with_service_specific_lexical_identity() {
    let arn = "arn:aws:lambda:us-east-1:123456789012:function:Example:live";
    let azure = "/subscriptions/00000000-1111-2222-3333-444444444444/resourceGroups/Example/providers/Microsoft.Compute/virtualMachines/VM1";
    let gcp = "//pubsub.googleapis.com/projects/example-project/topics/Example";
    let input = format!("{arn} {arn} arn:aws:s3:::example-bucket {azure} {gcp} https://management.azure.com{azure} //public.example/home/alice");
    let (text, count) = transform(
        &input,
        policy(&[Kind::CloudResource, Kind::Path, Kind::Host, Kind::Ip]),
    );
    assert_eq!(count, 5);
    let words: Vec<_> = text.split_whitespace().collect();
    assert_eq!(words[0], words[1]);
    assert!(words[..5].iter().all(|w| w.starts_with("RESOURCE_")));
    assert_eq!(words[5], format!("https://management.azure.com{azure}"));
    assert_eq!(words[6], "//public.example/home/alice");
    assert_eq!(
        transform(
            &format!("{arn} {azure} {gcp}"),
            policy(&[Kind::Path, Kind::Ip, Kind::Host])
        )
        .0,
        format!("{arn} {azure} {gcp}")
    );
    for invalid in [
        "arn:aws:s3:::*",
        "arn:aws:iam::1234:role/Example",
        "/subscriptions/not-a-guid/resourceGroups/demo",
        "//service.googleapis.com.evil/projects/demo",
    ] {
        assert_eq!(
            transform(invalid, policy(&[Kind::CloudResource])).0,
            invalid
        );
    }
}

#[test]
fn scoped_ip_is_never_partially_replaced_and_address_families_stay_distinct() {
    let unchanged = "[fe80::1%eth0!] [fe80::1%eth0:bad]";
    assert_eq!(transform(unchanged, policy(&[Kind::Ip])).0, unchanged);
    let (output, count) = transform("192.0.2.1 ::ffff:192.0.2.1", policy(&[Kind::Ip]));
    assert_eq!(count, 2);
    let words: Vec<_> = output.split_whitespace().collect();
    assert_ne!(words[0], words[1]);
}

#[test]
fn disabled_categories_do_not_scan_oversized_individual_url_tokens() {
    let text = format!("https://example.com/{}", "x".repeat(20_000));
    assert_eq!(transform(&text, InfrastructurePolicy::default()).0, text);
}

#[test]
fn explicit_domain_context_does_not_depend_on_the_host_category() {
    assert_eq!(
        transform(
            "domain=CORP.EXAMPLE dns_domain=corp.example host=corp.example",
            policy(&[Kind::Domain])
        )
        .0,
        "domain=DOMAIN_0001 dns_domain=DOMAIN_0001 host=corp.example"
    );
}

#[test]
fn versioned_corpus_matches_literal_expected_output() {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Corpus {
        schema_version: u32,
        rules_version: String,
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        id: String,
        categories: Vec<Kind>,
        input: String,
        expected: String,
        findings: usize,
    }
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/infrastructure-v1.json")).unwrap();
    assert_eq!(corpus.schema_version, 1);
    assert_eq!(
        corpus.rules_version,
        tgsum_core::infrastructure::RULES_VERSION
    );
    let mut positives = 0;
    let mut negatives = 0;
    for case in corpus.cases {
        let (text, count) = transform(&case.input, policy(&case.categories));
        assert_eq!(text, case.expected, "case {}", case.id);
        assert_eq!(count, case.findings, "case {}", case.id);
        if count > 0 {
            positives += 1
        } else {
            negatives += 1
        }
    }
    eprintln!("Synthetic infrastructure corpus: {positives} positive, {negatives} negative cases; all literal outputs match");
}

#[test]
fn project_reimports_keep_infrastructure_links_without_leaking_values_in_reports() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Repeated infrastructure").unwrap();
    let detector = InfrastructureDetector::new(policy(&[Kind::Ip, Kind::Host])).unwrap();
    let first = detector.scan("10.0.0.20 DB.LOCAL").unwrap();
    let assignment = store
        .assign_pseudonyms(&project.project_id, 0, &first.inputs())
        .unwrap();
    let old_ref = assignment.project.pseudonyms.as_ref().unwrap();
    let old = store.load_pseudonyms(&project.project_id, old_ref).unwrap();
    assert_eq!(first.apply(&old).unwrap().text, "IP_0001 HOST_0001");
    let next = detector.scan("db.local 10.0.0.1 10.0.0.20").unwrap();
    let reopened = ProjectStore::new(root.path());
    let assignment = reopened
        .assign_pseudonyms(
            &project.project_id,
            assignment.project.revision,
            &next.inputs(),
        )
        .unwrap();
    let mapping = reopened
        .load_pseudonyms(
            &project.project_id,
            assignment.project.pseudonyms.as_ref().unwrap(),
        )
        .unwrap();
    let output = next.apply(&mapping).unwrap();
    assert_eq!(output.text, "HOST_0001 IP_0002 IP_0001");
    let diagnostics = format!(
        "{:?} {}",
        output.findings,
        serde_json::to_string(&output.findings).unwrap()
    );
    assert!(!diagnostics.contains("db.local") && !diagnostics.contains("10.0.0.20"));
    assert_eq!(first.apply(&old).unwrap().text, "IP_0001 HOST_0001");
    assert!(next.apply(&old).is_err());
    for finding in &output.findings {
        assert!(output.text.is_char_boundary(finding.output.start));
        assert!(output.text.is_char_boundary(finding.output.end));
    }
}

#[test]
fn budgets_fail_explicitly_and_unicode_spans_remain_exact() {
    let detector =
        InfrastructureDetector::new(policy(&[Kind::Ip, Kind::Path, Kind::Host])).unwrap();
    assert!(detector.scan(&"x".repeat(16 * 1024 * 1024 + 1)).is_err());
    assert!(detector.scan(&"192.0.2.1 ".repeat(100_001)).is_err());
    assert!(detector
        .scan(&format!("/home/{}", "x".repeat(16 * 1024)))
        .is_err());
    let bad = InfrastructurePolicy {
        hostnames: vec!["https://PRIVATE_CONFIG.example/path".into()],
        ..Default::default()
    };
    let error = InfrastructureDetector::new(bad).err().unwrap();
    assert!(!error.to_string().contains("PRIVATE_CONFIG"));
    let too_many = InfrastructurePolicy {
        hostnames: vec!["host".into(); 257],
        ..Default::default()
    };
    assert!(InfrastructureDetector::new(too_many).is_err());
    assert_eq!(
        transform(
            "Журнал: 192.0.2.1 → db.local",
            policy(&[Kind::Ip, Kind::Host])
        )
        .0,
        "Журнал: IP_0001 → HOST_0001"
    );
}

#[test]
fn private_ipv4_mapped_url_is_classified_without_merging_standalone_families() {
    assert_eq!(
        transform("http://[::ffff:10.0.0.1]/log", policy(&[Kind::Url])).0,
        "URL_0001"
    );
    assert_eq!(
        transform("http://[::ffff:8.8.8.8]/log", policy(&[Kind::Url])).0,
        "http://[::ffff:8.8.8.8]/log"
    );
}

#[test]
fn quoted_usernames_are_replaced_as_complete_values() {
    assert_eq!(
        transform(
            r#"user="ACME\Alice Doe" login='ACME\Alice Doe'"#,
            policy(&[Kind::Username])
        )
        .0,
        r#"user="USER_0001" login='USER_0001'"#
    );
}
