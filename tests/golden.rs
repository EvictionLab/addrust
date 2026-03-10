use addrust::pipeline::{Pipeline, PipelineConfig};
use addrust::tables::build_rules;
use addrust::tables::abbreviations::ABBR;

fn pipeline() -> Pipeline {
    Pipeline::new(build_rules(&ABBR, &std::collections::HashMap::new()), &PipelineConfig::default())
}

/// Parse golden.csv and compare each address against expected output.
/// Format: input,street_number,pre_direction,street_name,suffix,post_direction,unit,po_box
#[test]
fn test_golden_dataset() {
    let csv = include_str!("../data/golden.csv");
    let p = pipeline();
    let mut failures = Vec::new();

    for (line_num, line) in csv.lines().enumerate().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.is_empty() {
            continue;
        }

        let input = cols[0];
        let expected_number = cols.get(1).copied().unwrap_or("").trim();
        let expected_pre_dir = cols.get(2).copied().unwrap_or("").trim();
        let expected_name = cols.get(3).copied().unwrap_or("").trim();
        let expected_suffix = cols.get(4).copied().unwrap_or("").trim();
        let expected_post_dir = cols.get(5).copied().unwrap_or("").trim();
        let expected_unit = cols.get(6).copied().unwrap_or("").trim();
        let expected_po_box = cols.get(7).copied().unwrap_or("").trim();

        let addr = p.parse(input);

        let got_number = addr.street_number.as_deref().unwrap_or("");
        let got_pre_dir = addr.pre_direction.as_deref().unwrap_or("");
        let got_name = addr.street_name.as_deref().unwrap_or("");
        let got_suffix = addr.suffix.as_deref().unwrap_or("");
        let got_post_dir = addr.post_direction.as_deref().unwrap_or("");
        let got_unit = addr.unit.as_deref().unwrap_or("");
        let got_po_box = addr.po_box.as_deref().unwrap_or("");

        let mut diffs = Vec::new();

        if got_number != expected_number {
            diffs.push(format!("  number: got={:?} expected={:?}", got_number, expected_number));
        }
        if got_pre_dir != expected_pre_dir {
            diffs.push(format!("  pre_dir: got={:?} expected={:?}", got_pre_dir, expected_pre_dir));
        }
        if got_name != expected_name {
            diffs.push(format!("  name: got={:?} expected={:?}", got_name, expected_name));
        }
        if got_suffix != expected_suffix {
            diffs.push(format!("  suffix: got={:?} expected={:?}", got_suffix, expected_suffix));
        }
        if got_post_dir != expected_post_dir {
            diffs.push(format!("  post_dir: got={:?} expected={:?}", got_post_dir, expected_post_dir));
        }
        if got_unit != expected_unit {
            diffs.push(format!("  unit: got={:?} expected={:?}", got_unit, expected_unit));
        }
        if got_po_box != expected_po_box {
            diffs.push(format!("  po_box: got={:?} expected={:?}", got_po_box, expected_po_box));
        }

        if !diffs.is_empty() {
            failures.push(format!(
                "Line {}: {:?}\n{}",
                line_num + 1,
                input,
                diffs.join("\n")
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} golden test failures:\n\n{}\n",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

#[test]
fn debug_st_james() {
    let prepped = addrust::prepare::prepare("42 W St James Pl");
    eprintln!("prepared: {:?}", prepped);
    let p = pipeline();
    let addr = p.parse("42 W St James Pl");
    eprintln!("number: {:?}", addr.street_number);
    eprintln!("pre_dir: {:?}", addr.pre_direction);
    eprintln!("name: {:?}", addr.street_name);
    eprintln!("suffix: {:?}", addr.suffix);
    eprintln!("warnings: {:?}", addr.warnings);
    assert_eq!(addr.street_number.as_deref(), Some("42"));
}
