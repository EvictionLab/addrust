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
[dictionaries.suffix_all]
add = [{ short = "PSGE", long = "PASSAGE" }]

[dictionaries.suffix_common]
add = [{ short = "PSGE", long = "PASSAGE" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Psge");
    assert_eq!(addr.suffix.as_deref(), Some("PASSAGE"));
}

#[test]
fn test_config_adds_custom_na_value() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.na_values]
add = [{ short = "VACANT", long = "" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("VACANT");
    assert!(addr.warnings.contains(&"change_na_address".to_string()));
}

#[test]
fn test_config_removes_na_value() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.na_values]
remove = ["NULL"]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("NULL");
    // NULL should no longer trigger NA warning
    assert!(!addr.warnings.contains(&"change_na_address".to_string()));
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

#[test]
fn test_mt_to_mount_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 MT VERNON AVE");
    assert_eq!(addr.street_name.as_deref(), Some("MOUNT VERNON"));
}

#[test]
fn test_ft_to_fort_default() {
    let p = Pipeline::default();
    let addr = p.parse("456 FT WORTH BLVD");
    assert_eq!(addr.street_name.as_deref(), Some("FORT WORTH"));
}

#[test]
fn test_config_adds_street_name_abbr() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.street_name_abbr]
add = [{ short = "PT", long = "POINT" }]
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 PT LOOKOUT RD");
    assert_eq!(addr.street_name.as_deref(), Some("POINT LOOKOUT"));
}

#[test]
fn test_full_pipeline_with_tables_cleanup() {
    let p = Pipeline::default();

    // NA values from table
    let addr = p.parse("NULL");
    assert!(addr.warnings.contains(&"change_na_address".to_string()));

    let addr = p.parse("UNKNOWN");
    assert!(addr.warnings.contains(&"change_na_address".to_string()));

    // Street name abbreviations from table
    let addr = p.parse("123 MT PLEASANT AVE");
    assert_eq!(addr.street_name.as_deref(), Some("MOUNT PLEASANT"));

    let addr = p.parse("456 FT HAMILTON PKWY");
    assert_eq!(addr.street_name.as_deref(), Some("FORT HAMILTON"));

    // ST → SAINT still works (hardcoded rule)
    let addr = p.parse("789 ST MARKS PL");
    assert_eq!(addr.street_name.as_deref(), Some("SAINT MARKS"));
}

#[test]
fn test_output_suffix_short() {
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Street");
    assert_eq!(addr.suffix.as_deref(), Some("ST"));
}

#[test]
fn test_output_suffix_long_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St");
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_output_direction_long() {
    let config: Config = toml::from_str(
        r#"
[output]
direction = "long"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 N Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
}

#[test]
fn test_output_direction_short_default() {
    let p = Pipeline::default();
    let addr = p.parse("123 North Main St");
    assert_eq!(addr.pre_direction.as_deref(), Some("N"));
}

#[test]
fn test_output_suffix_variant_canonicalizes() {
    // DRIV is a variant spelling — should canonicalize to DR then expand to DRIVE (default long)
    let p = Pipeline::default();
    let addr = p.parse("123 Main Driv");
    assert_eq!(addr.suffix.as_deref(), Some("DRIVE"));
}

#[test]
fn test_output_suffix_variant_to_short() {
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Driv");
    assert_eq!(addr.suffix.as_deref(), Some("DR"));
}

#[test]
fn test_output_combined_settings() {
    let config: Config = toml::from_str(
        r#"
[output]
suffix = "short"
direction = "long"
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 N Main Drive");
    assert_eq!(addr.suffix.as_deref(), Some("DR"));
    assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
}
