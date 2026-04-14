/// Declare all address columns in one place.
/// Generates: Address struct fields, Col enum, field()/field_mut(), COL_DEFS.
macro_rules! define_columns {
    ( $( $variant:ident, $field:ident, $key:literal, $label:literal );+ $(;)? ) => {
        /// The parsed result of an address string.
        /// Each field is `None` until extracted by a pipeline step.
        #[derive(Debug, Default, Clone)]
        pub struct Address {
            $( pub $field: Option<String>, )+
            pub warnings: Vec<String>,
        }

        /// Which column of a parsed address a step targets.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Col {
            $( $variant, )+
        }

        pub struct ColDef {
            pub col: Col,
            pub key: &'static str,
            pub label: &'static str,
        }

        pub const COL_DEFS: &[ColDef] = &[
            $( ColDef { col: Col::$variant, key: $key, label: $label }, )+
        ];

        impl Address {
            /// Get a mutable reference to a field by enum variant.
            pub fn field_mut(&mut self, col: Col) -> &mut Option<String> {
                match col {
                    $( Col::$variant => &mut self.$field, )+
                }
            }

            /// Get a reference to a field by enum variant.
            pub fn field(&self, col: Col) -> &Option<String> {
                match col {
                    $( Col::$variant => &self.$field, )+
                }
            }
        }
    };
}

define_columns! {
    StreetNumber,  street_number,  "street_number",  "Street Number";
    PreDirection,  pre_direction,  "pre_direction",  "Pre-Direction";
    StreetName,    street_name,    "street_name",    "Street Name";
    Suffix,        suffix,         "suffix",         "Suffix";
    PostDirection, post_direction, "post_direction",  "Post-Direction";
    Unit,          unit,           "unit",           "Unit";
    UnitType,      unit_type,      "unit_type",      "Unit Type";
    PoBox,         po_box,         "po_box",         "PO Box";
    Building,      building,       "building",       "Building";
    BuildingType,  building_type,  "building_type",  "Building Type";
    ExtraFront,    extra_front,    "extra_front",    "Extra Front";
    ExtraBack,     extra_back,     "extra_back",     "Extra Back";
    City,          city,           "city",           "City";
    State,         state,          "state",          "State";
    Zip,           zip,            "zip",            "Zip";
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
}

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
