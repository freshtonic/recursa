use recursa_example_json::{grammar, parse};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r#"{"answer":[true,null,42]}"#.to_owned());
    let value = parse(&source)?;
    let shape = match value {
        grammar::Value::Object(_) => "object",
        grammar::Value::Array(_) => "array",
        grammar::Value::String(_) => "string",
        grammar::Value::Number(_) => "number",
        grammar::Value::Bool(_) => "boolean",
        grammar::Value::Null => "null",
    };
    println!("parsed JSON {shape}");
    Ok(())
}
