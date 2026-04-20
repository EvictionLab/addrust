use addrust::config::Config;
use addrust::pipeline::Pipeline;

#[test]
fn test_config_disables_suffix_steps() {
    let config: Config = toml::from_str(
        r#"
[steps]
disabled = ["suffix_common", "suffix_all"]
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
[dictionaries.suffix]
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
    // NA rewrite empties the working string; no fields extracted
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());
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
    // NULL is no longer an NA value, so it should be parsed (becomes street_name)
    assert!(addr.street_name.is_some());
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
fn test_config_adds_street_name() {
    let config: Config = toml::from_str(
        r#"
[dictionaries.street_name]
add = [{ short = "PT", long = "POINT", tags = ["name"] }]
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

    // NA values from table — rewrite empties working string, no fields extracted
    let addr = p.parse("NULL");
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());

    let addr = p.parse("UNKNOWN");
    assert!(addr.street_name.is_none());
    assert!(addr.street_number.is_none());

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
fn test_output_suffix_variant() {
    // DRIV is a variant spelling — canonicalizes to DRIVE (long) or DR (short)
    let p = Pipeline::default();
    let addr = p.parse("123 Main Driv");
    assert_eq!(addr.suffix.as_deref(), Some("DRIVE"));

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

#[test]
fn test_custom_step_extracts_po_box_digits() {
    // Custom step must be placed early (before extra_front/street_number consume the input)
    let config: Config = toml::from_str(
        r#"
[steps]
step_order = ["na_check", "city_state_zip", "po_box", "custom_po_box_digits"]

[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)\b'
output_col = "po_box"
skip_if_filled = true
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("BOX 456");
    assert!(addr.po_box.is_some(), "po_box should be extracted by custom step");
}

#[test]
fn test_custom_step_respects_step_order() {
    let config: Config = toml::from_str(
        r#"
[steps]
step_order = ["na_check", "custom_rewrite_test", "city_state_zip"]

[[steps.custom_steps]]
type = "rewrite"
label = "custom_rewrite_test"
pattern = '\bTEST\b'
replacement = 'TESTED'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.steps();
    assert_eq!(summaries[0].label(), "na_check");
    assert_eq!(summaries[1].label(), "custom_rewrite_test");
    assert_eq!(summaries[2].label(), "city_state_zip");
}

#[test]
fn test_custom_step_can_be_disabled() {
    let config: Config = toml::from_str(
        r#"
[steps]
disabled = ["custom_po_box_digits"]

[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)\b'
output_col = "po_box"
skip_if_filled = true
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.steps();
    let custom = summaries.iter().find(|s| s.label() == "custom_po_box_digits").unwrap();
    assert!(!custom.enabled());
}

#[test]
fn test_custom_rewrite_step() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "rewrite"
label = "custom_normalize_hwy"
pattern = '\bHIGHWAY\b'
replacement = 'HWY'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 HIGHWAY 50");
    // After custom rewrite HWY, then highway_number_to_word converts 50 → FIFTY
    assert_eq!(addr.street_name.as_deref(), Some("HWY FIFTY"));
}

#[test]
fn test_custom_steps_config_roundtrip() {
    let toml_str = r#"
[steps]
step_order = ["na_check", "custom_box", "po_box"]

[[steps.custom_steps]]
type = "extract"
label = "custom_box"
pattern = '\bBOX (\d+)\b'
output_col = "po_box"
skip_if_filled = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.steps.custom_steps.len(), 1);

    let serialized = config.to_toml();
    let reparsed: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.steps.custom_steps.len(), 1);
    assert_eq!(reparsed.steps.custom_steps[0].label, "custom_box");
    assert_eq!(reparsed.steps.step_order, vec!["na_check", "custom_box", "po_box"]);
}

#[test]
fn test_unit_type_extracted() {
    let p = Pipeline::default();
    let addr = p.parse("123 Main St Apt 4B");
    assert_eq!(addr.unit_type.as_deref(), Some("APT"));
    assert_eq!(addr.unit.as_deref(), Some("4B"));
}

#[test]
fn test_highway_number_to_word() {
    let p = Pipeline::default();
    // HIGHWAY 66 → highway_number_to_word fires → "HIGHWAY SIXTYSIX"
    let addr = p.parse("123 HIGHWAY 66");
    assert_eq!(addr.street_name.as_deref(), Some("HIGHWAY SIXTYSIX"));
    assert_eq!(addr.street_number.as_deref(), Some("123"));
}

#[test]
fn test_ordinal_to_word() {
    let p = Pipeline::default();
    // 42ND → ordinal_to_word fires → "FORTYSECOND"
    let addr = p.parse("123 42ND ST");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("FORTYSECOND"));
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_fractional_road() {
    let p = Pipeline::default();
    // After street_number extracts "123", working is "8 1/2 MILE RD"
    // suffix extracts "RD" → "ROAD", working is "8 1/2 MILE"
    // fractional_road fires: "8 1/2" → "EIGHT AND ONEHALF"
    // street_name becomes "EIGHT AND ONEHALF MILE"
    let addr = p.parse("123 8 1/2 MILE RD");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("EIGHT AND ONEHALF MILE"));
    assert_eq!(addr.suffix.as_deref(), Some("ROAD"));
}


#[test]
fn test_invalid_custom_step_skipped_gracefully() {
    let config: Config = toml::from_str(
        r#"
[[steps.custom_steps]]
type = "extract"
label = "bad_step"
pattern = '(?P<unclosed'
output_col = "po_box"
"#,
    )
    .unwrap();
    // Should not panic — invalid step is skipped with warning
    let p = Pipeline::from_config(&config);
    let summaries = p.steps();
    assert!(!summaries.iter().any(|s| s.label() == "bad_step"));
    // Default steps still work
    let addr = p.parse("123 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
}

#[test]
fn test_multiple_ordinals() {
    // Both ordinal tokens converted: 1ST → FIRST, 2ND → SECOND
    let p = Pipeline::default();
    let addr = p.parse("123 1ST AND 2ND ST");
    let name = addr.street_name.as_deref().unwrap();
    assert!(name.contains("FIRST"), "Expected FIRST in {:?}", name);
    assert!(name.contains("SECOND"), "Expected SECOND in {:?}", name);
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_bare_zero_street_number_preserved() {
    // Street number "0" is a valid address number and must not be stripped
    let p = Pipeline::default();
    let addr = p.parse("0 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("0"));
    assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
}

#[test]
fn test_highway_one_not_extracted_as_number() {
    // "HIGHWAY 1" — number-to-word runs early, converting to "HIGHWAY ONE"
    // before extra_front can split it. No street_number extracted.
    let p = Pipeline::default();
    let addr = p.parse("HIGHWAY 1");
    assert!(addr.street_number.is_none(), "street_number should be None, got {:?}", addr.street_number);
    assert_eq!(addr.street_name.as_deref(), Some("HIGHWAY ONE"));
}

#[test]
fn test_county_road_number() {
    // Trailing "5" after COUNTY ROAD is converted to FIVE by highway_number_to_word.
    // ROAD is treated as part of the street name here (not extracted as suffix),
    // because the highway_number_to_word pattern matches the full "COUNTY ROAD N" phrase.
    let p = Pipeline::default();
    let addr = p.parse("123 COUNTY ROAD 5");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("COUNTY ROAD FIVE"));
    // Suffix is None — ROAD stays embedded in the street name for county road addresses
    assert!(addr.suffix.is_none());
}

#[test]
fn test_ordinal_street() {
    // Pre-direction, ordinal street name, and suffix all extracted correctly
    let p = Pipeline::default();
    let addr = p.parse("123 W 42ND ST");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.pre_direction.as_deref(), Some("W"));
    assert_eq!(addr.street_name.as_deref(), Some("FORTYSECOND"));
    assert_eq!(addr.suffix.as_deref(), Some("STREET"));
}

#[test]
fn test_source_rewrite_no_side_effects() {
    // "#007" → # captured into unit_type, "007" → leading zeros stripped → "7"
    let p = Pipeline::default();
    let addr = p.parse("123 Main St #007");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
    assert_eq!(addr.unit.as_deref(), Some("7"));
    assert_eq!(addr.unit_type.as_deref(), Some("#"));
}

#[test]
fn test_step_overrides_deserialize() {
    let config: Config = toml::from_str(
        r#"
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
skip_if_filled = false

[steps.step_overrides.unit_type_value]
output_col = "unit"
"#,
    )
    .unwrap();
    assert_eq!(config.steps.step_overrides.len(), 2);
    let po_box = &config.steps.step_overrides["po_box"];
    assert_eq!(po_box.pattern.as_deref(), Some(r"\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b"));
    assert_eq!(po_box.skip_if_filled, Some(false));
    let utv = &config.steps.step_overrides["unit_type_value"];
    assert!(matches!(&utv.output_col, Some(addrust::step::OutputCol::Single(s)) if s == "unit"));
    assert!(utv.pattern.is_none());
}

#[test]
fn test_step_overrides_applied_in_pipeline() {
    let config: Config = toml::from_str(
        r#"
[steps.step_overrides.po_box]
pattern = '\b(?:P\W*O\W*BO?X|POB)\W*(\w+(?:-\d)?)\b'
"#,
    )
    .unwrap();
    let p = Pipeline::from_config(&config);
    // The dash-digit variant should now be captured
    let addr = p.parse("PO BOX 123-4");
    assert_eq!(addr.po_box.as_deref(), Some("PO BOX 123-4"));
}

#[test]
fn test_prepare_steps_in_pipeline() {
    let config = Config::default();
    let p = Pipeline::from_config(&config);
    let summaries = p.steps();
    // First steps should be prepare rules
    assert_eq!(summaries[0].label(), "fix_ampersand");
    assert_eq!(summaries[0].step_type(), "rewrite");
}


