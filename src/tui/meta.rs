pub struct StepTypeMeta {
    pub name: &'static str,
    pub display: &'static str,
}

pub const STEP_TYPES: &[StepTypeMeta] = &[
    StepTypeMeta {
        name: "extract",
        display: "Extract",
    },
    StepTypeMeta {
        name: "rewrite",
        display: "Rewrite",
    },
    StepTypeMeta {
        name: "standardize",
        display: "Standardize",
    },
];

pub fn find_step_type(name: &str) -> Option<&'static StepTypeMeta> {
    STEP_TYPES.iter().find(|m| m.name == name)
}

pub const TABLE_DESCRIPTIONS: &[(&str, &str)] = &[
    ("direction", "N/S/E/W, NORTH/SOUTH/EAST/WEST"),
    ("unit_type", "APT/SUITE/UNIT etc."),
    ("unit_location", "FRONT/REAR/BASEMENT etc."),
    ("suffix_all", "All suffix variants (AVE/AV/AVEN -> AVENUE)"),
    ("suffix_common", "Common suffixes only"),
    ("state", "State abbreviations"),
    ("street_name_abbr", "Street name abbreviations (MT->MOUNT)"),
    ("na_values", "NA/N/A values"),
    ("number_cardinal", "1->ONE, 42->FORTYTWO"),
    ("number_ordinal", "1->FIRST, 42->FORTYSECOND"),
];
