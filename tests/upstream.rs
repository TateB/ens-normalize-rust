use ens_normalize::{EnsError, Result, Token, ens_normalize, ens_tokenize, nfc, nfd, str_from_cps};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct NormCase {
    name: String,
    norm: Option<String>,
    #[serde(default)]
    error: bool,
    comment: Option<String>,
}

fn load_cases(json: &str) -> Vec<NormCase> {
    serde_json::from_str(json).expect("valid upstream validation JSON")
}

fn case_label(case: &NormCase, i: usize) -> String {
    format!(
        "#{i} {:?} ({})",
        case.name,
        case.comment.as_deref().unwrap_or("no comment")
    )
}

fn assert_normalize_cases(cases: &[NormCase]) {
    for (i, case) in cases.iter().enumerate() {
        let result = ens_normalize(&case.name);
        if case.error {
            assert!(
                result.is_err(),
                "{} expected error, got {:?}",
                case_label(case, i),
                result
            );
        } else {
            let expected = case.norm.as_ref().unwrap_or(&case.name);
            assert_eq!(
                result.as_deref(),
                Ok(expected.as_str()),
                "{}",
                case_label(case, i)
            );
        }
    }
}

fn normalize_via_tokenize(name: &str) -> Result<String> {
    let mut cps = Vec::new();
    for token in ens_tokenize(name) {
        match token {
            Token::Disallowed { .. } => return Err(EnsError::new("disallowed")),
            Token::Ignored { .. } => {}
            Token::Stop { cp } => cps.push(cp),
            Token::Valid { cps: v }
            | Token::Mapped { cps: v, .. }
            | Token::Emoji { cps: v, .. }
            | Token::Nfc { cps: v, .. } => cps.extend(v),
        }
    }
    let norm = str_from_cps(&nfc(&cps))?;
    if ens_normalize(&norm)? != norm {
        return Err(EnsError::new(format!("wrong: {norm}")));
    }
    if norm.is_empty() {
        ens_normalize(name)?;
    }
    Ok(norm)
}

fn assert_tokenize_cases(cases: &[NormCase]) {
    for (i, case) in cases.iter().enumerate() {
        let result = normalize_via_tokenize(&case.name);
        if case.error {
            assert!(
                result.is_err(),
                "{} expected tokenization error, got {:?}",
                case_label(case, i),
                result
            );
        } else {
            let expected = case.norm.as_ref().unwrap_or(&case.name);
            assert_eq!(
                result.as_deref(),
                Ok(expected.as_str()),
                "{}",
                case_label(case, i)
            );
        }
    }
}

#[test]
fn upstream_validate_normalize() {
    let cases = load_cases(include_str!("fixtures/validate-tests.json"));
    assert_normalize_cases(&cases);
}

#[test]
fn upstream_custom_normalize() {
    let cases = load_cases(include_str!("fixtures/custom-tests.json"));
    assert_normalize_cases(&cases);
}

#[test]
fn upstream_validate_tokenize_equivalence() {
    let cases = load_cases(include_str!("fixtures/validate-tests.json"));
    assert_tokenize_cases(&cases);
}

#[test]
fn upstream_nf_tests() {
    let tests: BTreeMap<String, Vec<[String; 3]>> =
        serde_json::from_str(include_str!("fixtures/nf-tests.json")).expect("valid nf tests");
    for (section, cases) in tests {
        for (i, [input, want_nfd, want_nfc]) in cases.into_iter().enumerate() {
            let input = ens_normalize::explode_cp(&input);
            let got_nfd = str_from_cps(&nfd(&input)).unwrap();
            let got_nfc = str_from_cps(&nfc(&input)).unwrap();
            assert_eq!(got_nfd, want_nfd, "{section} #{i} nfd");
            assert_eq!(got_nfc, want_nfc, "{section} #{i} nfc");
        }
    }
}
