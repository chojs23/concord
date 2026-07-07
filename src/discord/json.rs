use std::collections::BTreeMap;

use serde_json::Value;

/// Collects the fields of a JSON object that are not in `known_fields`.
/// Parsers stash these so unrecognized Discord payload fields survive a
/// parse/serialize round trip instead of being dropped.
pub(crate) fn extra_fields(value: &Value, known_fields: &[&str]) -> BTreeMap<String, Value> {
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter(|(field, _)| !known_fields.contains(&field.as_str()))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}
