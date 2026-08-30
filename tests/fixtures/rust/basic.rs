use rust::helper;

pub struct RustThing;

pub trait RustTrait {}

pub mod nested {}

pub fn rust_entry() {
    helper();
}

impl RustThing {
    pub fn rust_method(&self) {}
}
