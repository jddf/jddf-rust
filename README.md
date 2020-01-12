# jddf [![][crates-badge]][crates-url] [![][ci-badge]][ci-url]

> Documentation on docs.rs: https://docs.rs/jddf

This crate is a Rust implementation of **JSON Data Definition Format**. You
can use it to:

1. Validate input data is valid against a schema,
2. Get a list of validation errors with that input data, or
3. Build your own custom tooling on top of JSON Data Definition Format.

[crates-badge]: https://img.shields.io/crates/v/jddf
[ci-badge]: https://github.com/jddf/jddf-rust/workflows/Rust%20CI/badge.svg?branch=master
[crates-url]: https://crates.io/crates/jddf
[ci-url]: https://github.com/jddf/jddf-rust/actions

## Usage

The [detailed documentation on docs.rs](https://docs.rs/jddf) goes into more
detail, but at a high level here's how you use this crate to validate inputted
data:

```rust
use serde_json::json;
use jddf::{Schema, SerdeSchema, Validator, ValidationError};
use failure::Error;
use std::collections::HashSet;

fn main() -> Result<(), Error> {
    let demo_schema_data = r#"
        {
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "float64" },
                "phones": {
                    "elements": { "type": "string" }
                }
            }
        }
    "#;

    // The SerdeSchema type is a serde-friendly format for representing
    // schemas.
    let demo_schema: SerdeSchema = serde_json::from_str(demo_schema_data)?;

    // The Schema type is a higher-level format that does more validity
    // checks.
    let demo_schema = Schema::from_serde(demo_schema).unwrap();

    // Validator can quickly check if an instance satisfies some schema.
    // With the new_with_config constructor, you can configure how many
    // errors to return, and how to handle the possibility of a
    // circularly-defined schema.
    let validator = Validator::new();
    let input_ok = json!({
        "name": "John Doe",
        "age": 43,
        "phones": [
            "+44 1234567",
            "+44 2345678"
        ]
    });

    let validation_errors_ok = validator.validate(&demo_schema, &input_ok)?;
    assert!(validation_errors_ok.is_empty());

    let input_bad = json!({
        "age": "43",
        "phones": [
            "+44 1234567",
            442345678
        ]
    });

    // Each ValidationError holds paths to the bad part of the input, as
    // well as the part of the schema which rejected it.
    //
    // For testing purposes, we'll sort the errors so that their order is
    // predictable.
    let mut validation_errors_bad = validator.validate(&demo_schema, &input_bad)?;
    validation_errors_bad.sort_by_key(|err| err.instance_path().to_string());
    assert_eq!(validation_errors_bad.len(), 3);

    // "name" is required
    assert_eq!(validation_errors_bad[0].instance_path().to_string(), "");
    assert_eq!(validation_errors_bad[0].schema_path().to_string(), "/properties/name");

    // "age" has the wrong type
    assert_eq!(validation_errors_bad[1].instance_path().to_string(), "/age");
    assert_eq!(validation_errors_bad[1].schema_path().to_string(), "/properties/age/type");

    // "phones[1]" has the wrong type
    assert_eq!(validation_errors_bad[2].instance_path().to_string(), "/phones/1");
    assert_eq!(validation_errors_bad[2].schema_path().to_string(), "/properties/phones/elements/type");

    Ok(())
}
```
