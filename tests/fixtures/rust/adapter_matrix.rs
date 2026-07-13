#![allow(dead_code)]

use std::ptr;

#[generated]
struct Γ {
    value: i32,
}

macro_rules! double {
    ($value:expr) => {
        $value * 2
    };
}

#[automatically_derived]
impl Γ {
    fn compute(&self, π: i32) -> i32 {
        // line comment
        let values = vec![1, 2];
        /* block comment */
        unsafe { ptr::read(&π) }
            + if self.value == 0 {
                double!(π)
            } else {
                values.len() as i32
            }
    }
}
