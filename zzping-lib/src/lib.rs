// Copyright 2021 Google LLC
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

//! This contains all the tooling for reading and writting messages of differnt
//! formats. This library is common to all programs of the zzping suite.

pub mod batchdata;
pub mod compress;
pub mod dynrmp;
pub mod framedata;
pub mod framedataq;
pub mod framestats;

/// This is a test macro that tries to do a dbg!() but inlined. Takes less space.
#[macro_export]
macro_rules! dbgf {
    () => {
        eprintln!("[{}:{}]", file!(), line!());
    };
    ($val:expr $(,)?) => {
        match $val {
            tmp => {
                eprintln!("[{}:{}] {} = {:?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbgf!($val)),+,)
    };
}

// Placeholder for tests
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let x = 2;
        assert_eq!(x + x, 4);
    }
}
