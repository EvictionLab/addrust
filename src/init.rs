use crate::pipeline::Pipeline;
use crate::tables::abbreviations::build_default_tables;

/// Generate a default .addrust.toml content string with all rules and tables.
pub fn generate_default_config() -> String {
    let mut out = String::new();

    out.push_str("# addrust pipeline configuration\n");
    out.push_str("# Uncomment and edit to customize parsing behavior.\n\n");

    // Rules section
    out.push_str("[rules]\n");
    out.push_str("# Disable individual rules by label:\n");
    out.push_str("# disabled = []\n");
    out.push_str("#\n");
    out.push_str("# Disable entire groups:\n");
    out.push_str("# disabled_groups = []\n");
    out.push_str("#\n");
    out.push_str("# Available rules (in pipeline order):\n");

    let pipeline = Pipeline::default();
    for rule in pipeline.rule_summaries() {
        out.push_str(&format!(
            "#   {:30} (group: {}, action: {:?})\n",
            rule.label, rule.group, rule.action
        ));
    }

    out.push('\n');

    // Dictionary sections
    let tables = build_default_tables();
    for name in tables.table_names() {
        let table = tables.get(name).unwrap();
        out.push_str(&format!("# [dictionaries.{}]\n", name));
        out.push_str(&format!("# {} entries. Examples:\n", table.entries.len()));
        for entry in table.entries.iter().take(3) {
            out.push_str(&format!("#   {} -> {}\n", entry.short, entry.long));
        }
        out.push_str("# add = [{ short = \"XX\", long = \"EXAMPLE\" }]\n");
        out.push_str("# remove = [\"VALUE\"]\n");
        out.push_str("# override = [{ short = \"XX\", long = \"NEW LONG\" }]\n");
        out.push('\n');
    }

    out
}
