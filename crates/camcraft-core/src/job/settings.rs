//! A setting named by its place in a job file, dotted (`passes.stepover`,
//! `linking.ramp_angle`, a strategy's own `order`, `size.0` for a place in a
//! list), read and written in what a thing serialises to: so one mechanism
//! serves every setting of every kind of thing, with nothing written for any
//! one field. The panels' defaults and the expressions settings are worked out
//! from both name settings so.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use serde::Serialize;
use serde::de::DeserializeOwned;
use toml::{Table, Value};

/// The setting at `path`, dotted, in `table`, a number in it the place in a
/// list (`size.0`): `None` where it is not written, as one at its default
/// often is not.
pub fn at<'t>(table: &'t Table, path: &str) -> Option<&'t Value> {
    let mut keys = path.split('.');
    let mut value = table.get(keys.next()?)?;
    for key in keys {
        value = match value {
            Value::Table(t) => t.get(key)?,
            Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

/// Set the setting at `path` in `table` to `value`, or take it out for `None`,
/// making the tables on the way to it.
pub fn put(table: &mut Table, path: &str, value: Option<Value>) {
    let mut keys: Vec<&str> = path.split('.').collect();
    let Some(last) = keys.pop() else {
        return;
    };
    let mut here = table;
    for (k, key) in keys.iter().enumerate() {
        // A place in a list is set where it is: the list is there, whole.
        if let Some(Value::Array(list)) = here.get_mut(*key) {
            let rest: Vec<&str> = keys.iter().skip(k + 1).copied().chain([last]).collect();
            put_in_list(list, &rest, value);
            return;
        }
        let next = here
            .entry((*key).to_string())
            .or_insert_with(|| Value::Table(Table::new()));
        let Some(t) = next.as_table_mut() else {
            return;
        };
        here = t;
    }
    match value {
        Some(v) => {
            here.insert(last.to_string(), v);
        }
        None => {
            here.remove(last);
        }
    }
}

/// Set the place `path` leads to in `list`, its first key the place: a list's
/// places are only ever set, never taken out.
fn put_in_list(list: &mut [Value], path: &[&str], value: Option<Value>) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    let Some(slot) = first.parse::<usize>().ok().and_then(|i| list.get_mut(i)) else {
        return;
    };
    match (rest.is_empty(), slot) {
        (true, slot) => {
            if let Some(v) = value {
                *slot = v;
            }
        }
        (false, Value::Table(t)) => put(t, &rest.join("."), value),
        (false, Value::Array(inner)) => put_in_list(inner, rest, value),
        _ => {}
    }
}

/// The kind of number a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Number {
    /// A length, an angle, a feed: any real number.
    Real,
    /// A count or a speed.
    Whole,
}

/// What kind of number the setting at `path` holds in a `T`, `table` one as
/// written, or `None` where no number setting is. Found by writing one there
/// and reading it back as written, since a setting at its default is not
/// written at all; and kept for each kind of `T` and each `variant` of it,
/// what decides which settings it has (an operation's strategy), so it is
/// found once.
pub fn number_kind<T: Serialize + DeserializeOwned>(
    table: &Table,
    path: &str,
    variant: &str,
) -> Option<Number> {
    type Found = HashMap<(&'static str, String, String), Option<Number>>;
    static FOUND: LazyLock<Mutex<Found>> = LazyLock::new(Mutex::default);
    let key = (
        std::any::type_name::<T>(),
        variant.to_string(),
        path.to_string(),
    );
    let known = FOUND
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .copied();
    if let Some(known) = known {
        return known;
    }
    let holds = |probe: Value| {
        let mut t = table.clone();
        put(&mut t, path, Some(probe.clone()));
        Value::Table(t)
            .try_into::<T>()
            .ok()
            .and_then(|back| Table::try_from(&back).ok())
            .is_some_and(|back| at(&back, path) == Some(&probe))
    };
    let kind = if holds(Value::Float(1234.5)) {
        Some(Number::Real)
    } else if holds(Value::Integer(1234)) {
        Some(Number::Whole)
    } else {
        None
    };
    FOUND
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, kind);
    kind
}
