use crate::pipeline::Pipeline;
use crate::tables::abbreviations::load_default_tables;

/// Generate a default .addrust.toml content string with all steps and tables.
pub fn generate_default_config() -> String {
    let mut out = String::new();

    out.push_str("# addrust pipeline configuration\n");
    out.push_str("# Uncomment and edit to customize parsing behavior.\n\n");

    // Steps section
    out.push_str("[steps]\n");
    out.push_str("# Disable individual steps by label:\n");
    out.push_str("# disabled = []\n");
    out.push_str("#\n");
    out.push_str("# Available steps (in pipeline order):\n");

    let pipeline = Pipeline::default();
    for step in pipeline.steps() {
        let template = step.pattern_template().unwrap_or("");
        out.push_str(&format!(
            "#   {:30} (type: {})\n",
            step.label(), step.step_type()
        ));
        if !template.is_empty() {
            out.push_str(&format!("#     pattern: {}\n", template));
        }
    }

    out.push('\n');

    // Dictionary sections
    let tables = load_default_tables();
    for name in tables.table_names() {
        let table = tables.get(name).unwrap();
        out.push_str(&format!("# [dictionaries.{}]\n", name));
        out.push_str(&format!("# {} entries. Examples:\n", table.groups.len()));
        for group in table.groups.iter().take(3) {
            out.push_str(&format!("#   {} -> {}\n", group.short, group.long));
        }
        out.push_str("# add = [{ short = \"XX\", long = \"EXAMPLE\" }]\n");
        out.push_str("# remove = [\"VALUE\"]\n");
        out.push_str("# override = [{ short = \"XX\", long = \"NEW LONG\" }]\n");
        out.push('\n');
    }

    out
}
