// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::hash::{Hash, Hasher};
use std::{cmp::Ordering, collections::HashMap};

use super::variant::Variant;

/*
  This allows Maps to get Hash, Ord, Eq; even if they don't support it.
  It might not be 100% safe, but it allows to create hashmaps with them as keys
  even if this is not even advisable. Avoids having two kinds of enums and
  complicating other stuff. You're not supposed to use these capabilities.
*/
#[derive(Eq, Default, Debug)]
pub struct Map {
    pub v: HashMap<Variant, Variant>,
}

impl Map {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_hashmap(other: HashMap<Variant, Variant>) -> Self {
        Self { v: other }
    }
    pub fn into_hashmap(self) -> HashMap<Variant, Variant> {
        self.v
    }
    pub fn to_vec(&self) -> Vec<(&Variant, &Variant)> {
        let mut items: Vec<(&Variant, &Variant)> = self.v.iter().collect();
        items.sort();
        items
    }
}

impl Hash for Map {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "!!Map!!".hash(state);
        for (k, v) in self.to_vec() {
            k.hash(state);
            v.hash(state);
        }
    }
}

impl PartialEq for Map {
    fn eq(&self, other: &Self) -> bool {
        self.to_vec() == other.to_vec()
    }
}

impl PartialOrd for Map {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Map {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_vec().cmp(&other.to_vec())
    }
}
