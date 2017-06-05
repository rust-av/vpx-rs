// TODO do w/out the unions?
#![feature(untagged_unions)]

pub mod vpx;

#[cfg(test)]
mod tests {
    use super::vpx::*;
    use std::mem;
    #[test]
    fn init_and_version() {

    }
}
