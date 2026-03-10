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
    pub fn field_mut(&mut self, field: Field) -> &mut Option<String> {
        match field {
            Field::StreetNumber => &mut self.street_number,
            Field::PreDirection => &mut self.pre_direction,
            Field::StreetName => &mut self.street_name,
            Field::Suffix => &mut self.suffix,
            Field::PostDirection => &mut self.post_direction,
            Field::Unit => &mut self.unit,
            Field::UnitType => &mut self.unit_type,
            Field::PoBox => &mut self.po_box,
            Field::Building => &mut self.building,
            Field::ExtraFront => &mut self.extra_front,
            Field::ExtraBack => &mut self.extra_back,
        }
    }

    /// Get a reference to a field by enum variant.
    pub fn field(&self, field: Field) -> &Option<String> {
        match field {
            Field::StreetNumber => &self.street_number,
            Field::PreDirection => &self.pre_direction,
            Field::StreetName => &self.street_name,
            Field::Suffix => &self.suffix,
            Field::PostDirection => &self.post_direction,
            Field::Unit => &self.unit,
            Field::UnitType => &self.unit_type,
            Field::PoBox => &self.po_box,
            Field::Building => &self.building,
            Field::ExtraFront => &self.extra_front,
            Field::ExtraBack => &self.extra_back,
        }
    }
}

/// Which field of an Address a step targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
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
