use chigiri_core::FieldError;

use super::{ResolvedValue, output_leaf, output_list};

#[test]
fn list_item_errors_preserve_the_same_index_in_successful_outer_values() {
    let resolved = output_list(vec![1_i32, 2, 3], |value| {
        if value == 2 {
            Err(FieldError::public("item error"))
        } else {
            output_leaf(value)
        }
    })
    .unwrap();

    let ResolvedValue::List(items) = resolved else {
        panic!("The output of the list must be ResolvedValue::List.");
    };
    assert_eq!(items.len(), 3);
    assert!(items[0].is_ok());
    assert!(items[1].is_err());
    assert!(items[2].is_ok());
}
