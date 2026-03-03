// scon/src/lib.rs
// S.C.O.N. — Schema-Compact Object Notation

pub mod value;
pub mod encoder;
pub mod decoder;
pub mod minifier;

pub use value::Value;
pub use encoder::Encoder;
pub use decoder::Decoder;
pub use minifier::Minifier;

// Convenience functions

pub fn encode(data: &Value) -> String {
    Encoder::new().encode(data)
}

pub fn encode_to(data: &Value, buf: &mut String) {
    Encoder::new().encode_to(data, buf);
}

pub fn encode_with_indent(data: &Value, indent: usize) -> String {
    Encoder::new().with_indent(indent).encode(data)
}

pub fn decode(input: &str) -> Result<Value, String> {
    Decoder::new().decode(input)
}

pub fn minify(scon: &str) -> String {
    Minifier::minify(scon)
}

pub fn expand(minified: &str, indent: usize) -> String {
    Minifier::expand(minified, indent)
}
