//! Every enum field lists its values in the schema, and its default is one
//! of them (contexts.md §4, domain 9).

use iron_document::facet::FacetKind;
use iron_document::value::ValueKind;

#[test]
fn every_enum_field_lists_its_values_including_its_default() {
    for kind in FacetKind::ALL {
        let default = kind.default_facet();
        for (field, vk) in kind.fields() {
            let options = kind.options(field);
            if *vk == ValueKind::Enum {
                let options = options.unwrap_or_else(|| panic!("{}.{field} lists no options", kind.name()));
                let value = default.get(field).and_then(|s| s.constant()).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
                assert!(options.contains(&value.as_str()), "{}.{field}: default {value} not in {options:?}", kind.name());
            } else {
                assert!(options.is_none(), "{}.{field} is not an enum but lists options", kind.name());
            }
        }
    }
}
