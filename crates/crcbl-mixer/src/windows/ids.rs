//! Session-stable numbers for what Windows names by string.
//!
//! [`DeviceId`](crate::DeviceId) and [`StreamId`](crate::StreamId) are
//! numbers, while Windows names an endpoint by its id string
//! (`{0.0.0.00000000}.{…}`) and an audio session by its instance identifier.
//! A [`Table`] keeps each object the mixer follows under its string, beside the
//! number it was handed the first time it was seen: the number stays the same
//! for as long as the object does, an object seen for the first time gets one
//! no live object has, and an object that goes takes its number with it.
//! Numbers mean nothing to another [`Mixer`](crate::Mixer), as on Linux.

use std::collections::BTreeMap;

/// The objects of one kind the mixer follows, each under its Windows name and
/// with its number.
#[derive(Debug)]
pub(super) struct Table<T> {
    entries: BTreeMap<String, (u32, T)>,
    /// The number the next new object is handed, unless a live one has it.
    next: u32,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next: 0,
        }
    }
}

impl<T> Table<T> {
    /// The number of the object named `key`, making it with `make` and
    /// handing it a new number when it is not here yet. Nothing is kept when
    /// `make` fails.
    pub(super) fn get_or_try_insert<E>(
        &mut self,
        key: &str,
        make: impl FnOnce() -> Result<T, E>,
    ) -> Result<u32, E> {
        if let Some((id, _)) = self.entries.get(key) {
            return Ok(*id);
        }
        let value = make()?;
        // Past `u32::MAX` new objects the count wraps; a number a live object
        // still holds is skipped, so two never share one.
        while self.entries.values().any(|(id, _)| *id == self.next) {
            self.next = self.next.wrapping_add(1);
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.entries.insert(key.to_owned(), (id, value));
        Ok(id)
    }

    /// Drops every object whose name `keep` rejects, with its number.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&str, &T) -> bool) {
        self.entries.retain(|key, (_, value)| keep(key, value));
    }

    /// The object numbered `id`, with its name.
    pub(super) fn by_id(&self, id: u32) -> Option<(&str, &T)> {
        self.entries
            .iter()
            .find(|(_, (candidate, _))| *candidate == id)
            .map(|(key, (_, value))| (key.as_str(), value))
    }

    /// Every object: its name, number and value.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, u32, &T)> {
        self.entries
            .iter()
            .map(|(key, (id, value))| (key.as_str(), *id, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(table: &mut Table<&'static str>, key: &'static str) -> u32 {
        table
            .get_or_try_insert(key, || Ok::<_, ()>(key))
            .expect("made")
    }

    /// **A number lasts as long as its object, and a new object gets a new
    /// one**: the same names seen again keep their numbers, a gone name is
    /// dropped, and a name that comes back after going is a new object.
    #[test]
    fn a_number_lasts_as_long_as_its_object() {
        let mut table = Table::default();
        let speakers = insert(&mut table, "speakers");
        let headset = insert(&mut table, "headset");
        assert_ne!(speakers, headset);
        assert_eq!(insert(&mut table, "speakers"), speakers, "seen again");
        assert_eq!(insert(&mut table, "headset"), headset, "seen again");

        table.retain(|key, _| key != "headset");
        assert!(table.by_id(headset).is_none(), "a gone object is dropped");
        assert_eq!(table.by_id(speakers), Some(("speakers", &"speakers")));

        let back = insert(&mut table, "headset");
        assert_ne!(back, headset, "back after going is a new object");
        assert_ne!(back, speakers);
        assert_eq!(table.iter().count(), 2);
    }

    #[test]
    fn a_failed_make_keeps_nothing_and_a_wrapped_count_skips_live_numbers() {
        let mut table: Table<&str> = Table::default();
        assert_eq!(
            table.get_or_try_insert("gone", || Err("refused")),
            Err("refused")
        );
        assert_eq!(table.iter().count(), 0);

        let first = insert(&mut table, "first");
        table.next = first;
        let second = insert(&mut table, "second");
        assert_ne!(second, first, "the live number is skipped");
    }
}
