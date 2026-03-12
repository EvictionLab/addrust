use crate::step::StepDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKey {
    Pattern,
    Table,
    OutputCol,
    Replacement,
    SkipIfFilled,
    Mode,
    InputCol,
    Label,
}

pub struct StepTypeMeta {
    pub name: &'static str,
    pub display: &'static str,
    pub visible: &'static [PropKey],
    pub required: fn(&StepDef) -> bool,
}

pub const STEP_TYPES: &[StepTypeMeta] = &[
    StepTypeMeta {
        name: "extract",
        display: "Extract",
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::OutputCol,
                   PropKey::SkipIfFilled, PropKey::Replacement, PropKey::InputCol,
                   PropKey::Label],
        required: |def| (def.pattern.is_some() || def.table.is_some())
            && def.output_col.is_some(),
    },
    StepTypeMeta {
        name: "rewrite",
        display: "Rewrite",
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::Replacement,
                   PropKey::InputCol, PropKey::Label],
        required: |def| def.pattern.is_some()
            && (def.replacement.is_some() || def.table.is_some()),
    },
    StepTypeMeta {
        name: "standardize",
        display: "Standardize",
        visible: &[PropKey::Pattern, PropKey::Table, PropKey::Replacement,
                   PropKey::OutputCol, PropKey::Mode, PropKey::Label],
        required: |def| def.output_col.is_some()
            && (def.table.is_some() || (def.pattern.is_some() && def.replacement.is_some())),
    },
];

pub const PROP_HELP: &[(PropKey, &str)] = &[
    (PropKey::Pattern, "Regex pattern to match. Use {table_name} for table references."),
    (PropKey::Table, "Abbreviation table for lookups."),
    (PropKey::OutputCol, "Output column(s) to write the match to."),
    (PropKey::Replacement, "Replacement text. Use $1, $2 for capture groups."),
    (PropKey::SkipIfFilled, "Skip this step if the output column already has a value."),
    (PropKey::Mode, "Match mode: whole field or per word."),
    (PropKey::InputCol, "Read from this column instead of the working string."),
    (PropKey::Label, "Unique identifier for this step."),
];

pub fn find_step_type(name: &str) -> Option<&'static StepTypeMeta> {
    STEP_TYPES.iter().find(|m| m.name == name)
}

pub fn help_text(key: PropKey) -> &'static str {
    PROP_HELP.iter().find(|p| p.0 == key).map(|p| p.1).unwrap_or("")
}
