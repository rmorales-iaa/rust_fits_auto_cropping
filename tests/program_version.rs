#[path = "../build_support/version.rs"]
mod version;

#[test]
fn compile_version_uses_base_100_rollover() {
    assert_eq!(
        version::Version::parse("0.0.0")
            .unwrap()
            .increment()
            .to_string(),
        "0.0.1"
    );
    assert_eq!(
        version::Version::parse("0.0.99")
            .unwrap()
            .increment()
            .to_string(),
        "0.1.0"
    );
    assert_eq!(
        version::Version::parse("0.99.99")
            .unwrap()
            .increment()
            .to_string(),
        "1.0.0"
    );
}

#[test]
fn version_template_requires_three_numeric_components() {
    assert!(version::Version::parse("0.100.0").is_err());
    assert!(version::Version::parse("0.0.100").is_err());
    assert!(version::Version::parse("1.2").is_err());
}
