use addrust::pipeline::Pipeline;

/// Parse golden.csv using the step-based pipeline and compare against expected output.
#[test]
fn test_golden_dataset_steps() {
    let p = Pipeline::from_steps_default();
    let csv = include_str!("../data/golden.csv");
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
            "\n{} golden test failures (step pipeline):\n\n{}\n",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

/// Regression test for the city_state_zip extraction. The main golden runner
/// above intentionally ignores city/state/zip, which let an off-by-one capture-
/// group bug ship in v0.1.2 (city alternation was non-capturing while
/// output_col referenced groups 1/2/3). These cases lock in the correct
/// extraction across the comma-anchored and start-anchored branches.
#[test]
fn test_city_state_zip_extraction() {
    let p = Pipeline::default();
    let cases = [
        // (input, expected_city, expected_state, expected_zip)
        (
            "123 Main St, Springfield IL 62704",
            Some("SPRINGFIELD"),
            Some("IL"),
            Some("62704"),
        ),
        (
            "MAIN ST, Winston Salem NC 12345",
            Some("WINSTON SALEM"),
            Some("NC"),
            Some("12345"),
        ),
        (
            "WINSTON-SALEM NC 12345",
            Some("WINSTON-SALEM"),
            Some("NC"),
            Some("12345"),
        ),
        (
            "123 Main St, Springfield IL 62704-1234",
            Some("SPRINGFIELD"),
            Some("IL"),
            Some("62704-1234"),
        ),
        (
            "123 Main St, Springfield IL 62704 US",
            Some("SPRINGFIELD"),
            Some("IL"),
            Some("62704"),
        ),
    ];

    let mut failures = Vec::new();
    for (input, want_city, want_state, want_zip) in cases {
        let addr = p.parse(input);
        let city = addr.city.as_deref();
        let state = addr.state.as_deref();
        let zip = addr.zip.as_deref();
        if city != want_city || state != want_state || zip != want_zip {
            failures.push(format!(
                "input: {:?}\n  city:  got={:?} want={:?}\n  state: got={:?} want={:?}\n  zip:   got={:?} want={:?}",
                input, city, want_city, state, want_state, zip, want_zip
            ));
        }
    }
    if !failures.is_empty() {
        panic!(
            "\n{} city_state_zip failures:\n\n{}\n",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

