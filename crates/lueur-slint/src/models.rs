//! Lists handed to the window, changed in place.
//!
//! Slint draws one element per row of a model. Handing it a new model each
//! time would make every row anew: a hovered card would lose its hover, a
//! focused one its focus, a running animation would start over. [`sync`]
//! changes only the rows that differ, and adds or removes rows at the end.
//!
//! A list inside a row (a setting's choices) is a model of its own, and two
//! models are equal only when they are the same one: [`reuse`] fills the
//! model the row already holds, so an unchanged row stays equal and is
//! left alone.

use slint::{Model, ModelRc, VecModel};

/// Make `model`'s rows `rows`, touching only those that change.
pub fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: impl IntoIterator<Item = T>) {
    let mut n = 0;
    for row in rows {
        if n < model.row_count() {
            if model.row_data(n).as_ref() != Some(&row) {
                model.set_row_data(n, row);
            }
        } else {
            model.push(row);
        }
        n += 1;
    }
    while model.row_count() > n {
        model.remove(model.row_count() - 1);
    }
}

/// A list property kept in step with `rows`: the first time it is handed a
/// model of its own, after that the rows of that model are changed in
/// place. `current` is what the property holds, `set` its setter.
pub fn keep<T: Clone + PartialEq + 'static>(
    current: ModelRc<T>,
    set: impl FnOnce(ModelRc<T>),
    rows: impl IntoIterator<Item = T>,
) {
    if let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() {
        sync(model, rows);
    } else {
        let model = VecModel::from(rows.into_iter().collect::<Vec<_>>());
        set(ModelRc::new(model));
    }
}

/// `rows` in the model `current` already is, when it is one of ours; a new
/// model otherwise. Hand back what it returns.
pub fn reuse<T: Clone + PartialEq + 'static>(current: &ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    if let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() {
        sync(model, rows);
        current.clone()
    } else {
        ModelRc::new(VecModel::from(rows))
    }
}

/// Row `i` of `model`, or the type's default when there is none yet.
pub fn old<T: Clone + Default + 'static>(model: &ModelRc<T>, i: usize) -> T {
    model.row_data(i).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_change_in_place() {
        let model = VecModel::from(vec![1, 2, 3]);
        sync(&model, [1, 5]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5]);
        sync(&model, [1, 5, 6, 7]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5, 6, 7]);
        sync(&model, []);
        assert_eq!(model.row_count(), 0);
    }

    #[test]
    fn a_list_in_a_row_stays_the_same_model() {
        let first = reuse(&ModelRc::default(), vec![1, 2]);
        let again = reuse(&first, vec![1, 3]);
        assert!(first == again);
        assert_eq!(again.iter().collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn a_property_gets_its_own_model_once() {
        let current: ModelRc<i32> = ModelRc::default();
        let mut handed = None;
        keep(current, |m| handed = Some(m), [1, 2]);
        let handed = handed.expect("a model of its own");
        assert_eq!(handed.row_count(), 2);
        keep(handed.clone(), |_| panic!("kept in place"), [3]);
        assert_eq!(handed.iter().collect::<Vec<_>>(), [3]);
    }
}
