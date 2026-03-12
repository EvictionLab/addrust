/// The parsed result of an address string.
/// Each field is `None` until extracted by a pipeline step.
#[derive(Debug, Default, Clone)]
pub struct Address {
    pub street_number: Option<String>,
    pub pre_direction: Option<String>,
    pub street_name: Option<String>,
    pub suffix: Option<String>,
    pub post_direction: Option<String>,
    pub unit: Option<String>,
    pub unit_type: Option<String>,
    pub po_box: Option<String>,
    pub building: Option<String>,
    pub extra_front: Option<String>,
    pub extra_back: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub zip: Option<String>,
    pub warnings: Vec<String>,
}

impl Address {
    /// Unite components into a clean address string.
    pub fn clean_address(&self) -> Option<String> {
        let parts: Vec<&str> = [
            self.po_box.as_deref(),
            self.street_number.as_deref(),
            self.pre_direction.as_deref(),
            self.street_name.as_deref(),
            self.suffix.as_deref(),
            self.post_direction.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    /// Street number + street name, for matching.
    pub fn short_address(&self) -> Option<String> {
        let parts: Vec<&str> = [
            self.po_box.as_deref(),
            self.street_number.as_deref(),
            self.street_name.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    /// Get a mutable reference to a field by enum variant.
    pub fn field_mut(&mut self, col: Col) -> &mut Option<String> {
        match col {
            Col::StreetNumber => &mut self.street_number,
            Col::PreDirection => &mut self.pre_direction,
            Col::StreetName => &mut self.street_name,
            Col::Suffix => &mut self.suffix,
            Col::PostDirection => &mut self.post_direction,
            Col::Unit => &mut self.unit,
            Col::UnitType => &mut self.unit_type,
            Col::PoBox => &mut self.po_box,
            Col::Building => &mut self.building,
            Col::ExtraFront => &mut self.extra_front,
            Col::ExtraBack => &mut self.extra_back,
            Col::City => &mut self.city,
            Col::State => &mut self.state,
            Col::Zip => &mut self.zip,
        }
    }

    /// Get a reference to a field by enum variant.
    pub fn field(&self, col: Col) -> &Option<String> {
        match col {
            Col::StreetNumber => &self.street_number,
            Col::PreDirection => &self.pre_direction,
            Col::StreetName => &self.street_name,
            Col::Suffix => &self.suffix,
            Col::PostDirection => &self.post_direction,
            Col::Unit => &self.unit,
            Col::UnitType => &self.unit_type,
            Col::PoBox => &self.po_box,
            Col::Building => &self.building,
            Col::ExtraFront => &self.extra_front,
            Col::ExtraBack => &self.extra_back,
            Col::City => &self.city,
            Col::State => &self.state,
            Col::Zip => &self.zip,
        }
    }
}

/// Which column of a parsed address a step targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Col {
    StreetNumber,
    PreDirection,
    StreetName,
    Suffix,
    PostDirection,
    Unit,
    UnitType,
    PoBox,
    Building,
    ExtraFront,
    ExtraBack,
    City,
    State,
    Zip,
}

pub struct ColDef {
    pub col: Col,
    pub key: &'static str,
    pub label: &'static str,
}

pub const COL_DEFS: &[ColDef] = &[
    ColDef { col: Col::StreetNumber,  key: "street_number",  label: "Street Number" },
    ColDef { col: Col::PreDirection,  key: "pre_direction",  label: "Pre-Direction" },
    ColDef { col: Col::StreetName,    key: "street_name",    label: "Street Name" },
    ColDef { col: Col::Suffix,        key: "suffix",         label: "Suffix" },
    ColDef { col: Col::PostDirection, key: "post_direction",  label: "Post-Direction" },
    ColDef { col: Col::Unit,          key: "unit",           label: "Unit" },
    ColDef { col: Col::UnitType,      key: "unit_type",      label: "Unit Type" },
    ColDef { col: Col::PoBox,         key: "po_box",         label: "PO Box" },
    ColDef { col: Col::Building,      key: "building",       label: "Building" },
    ColDef { col: Col::ExtraFront,    key: "extra_front",    label: "Extra Front" },
    ColDef { col: Col::ExtraBack,     key: "extra_back",     label: "Extra Back" },
    ColDef { col: Col::City,          key: "city",           label: "City" },
    ColDef { col: Col::State,         key: "state",          label: "State" },
    ColDef { col: Col::Zip,           key: "zip",            label: "Zip" },
];

impl Col {
    pub fn from_key(key: &str) -> Result<Col, String> {
        COL_DEFS.iter()
            .find(|d| d.key == key)
            .map(|d| d.col)
            .ok_or_else(|| format!("Unknown column name: {}", key))
    }

    pub fn key(&self) -> &'static str {
        COL_DEFS.iter().find(|d| d.col == *self).unwrap().key
    }

    pub fn label(&self) -> &'static str {
        COL_DEFS.iter().find(|d| d.col == *self).unwrap().label
    }
}

/// Mutable state during parsing.
#[derive(Debug)]
pub struct AddressState {
    /// The working string being consumed (equivalent to temp_address in R).
    pub working: String,
    /// Extracted components so far.
    pub fields: Address,
}

impl AddressState {
    pub fn new(input: &str) -> Self {
        Self {
            working: input.to_uppercase(),
            fields: Address::default(),
        }
    }

    /// Create from an already-prepared (uppercased, cleaned) string.
    pub fn new_from_prepared(prepared: String) -> Self {
        Self {
            working: prepared,
            fields: Address::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_col_from_key_roundtrip() {
        for def in COL_DEFS {
            assert_eq!(Col::from_key(def.key).unwrap(), def.col);
            assert_eq!(def.col.label(), def.label);
            assert_eq!(def.col.key(), def.key);
        }
    }

    #[test]
    fn test_col_from_key_unknown() {
        assert!(Col::from_key("nonsense").is_err());
    }
}
