use addrust::config::Config;
use addrust::pipeline::Pipeline;

#[test]
fn test_config_disables_suffix_group() {
    let config: Config = toml::from_str(
        r#"
[rules]
disabled_groups = ["suffix"]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert!(addr.suffix.is_none());
    // ST becomes part of the street name when suffix extraction is disabled
    assert!(addr.street_name.as_deref().unwrap().contains("ST"));
}

#[test]
fn test_config_adds_custom_suffix() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.all_suffix]
add = [{ short = "PSGE", long = "PASSAGE" }]

[dictionaries.common_suffix]
add = [{ short = "PSGE", long = "PASSAGE" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Psge");
    assert_eq!(addr.suffix.as_deref(), Some("PASSAGE"));
}

#[test]
fn test_default_pipeline_matches_no_config() {
    let default_p = Pipeline::default();
    let config_p = Pipeline::from_config(&Config::default());

    let addr1 = default_p.parse("123 N Main St Apt 4");
    let addr2 = config_p.parse("123 N Main St Apt 4");

    assert_eq!(addr1.street_number, addr2.street_number);
    assert_eq!(addr1.pre_direction, addr2.pre_direction);
    assert_eq!(addr1.street_name, addr2.street_name);
    assert_eq!(addr1.suffix, addr2.suffix);
    assert_eq!(addr1.unit, addr2.unit);
}
