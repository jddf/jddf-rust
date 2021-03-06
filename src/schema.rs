//! JDDF schema representations.
//!
//! This module provides both an abstract ([`Schema`](struct.Schema.html)) and a
//! serializable/deserializable ([`SerdeSchema`](struct.SerdeSchema.html))
//! representation of JDDF schemas.

use crate::errors::JddfError;
use failure::{bail, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// An abstract representation of a JDDF schema.
///
/// This struct is meant for use by validators, code generators, or other
/// high-level processors of schemas. For serialization and deserialization of
/// schemas, instead use [`Serde`](struct.Serde.html).
#[derive(Clone, PartialEq, Debug)]
pub struct Schema {
    defs: Option<HashMap<String, Schema>>,
    form: Box<Form>,
    extra: HashMap<String, Value>,
}

impl Schema {
    /// Construct a new schema from its constituent parts.
    ///
    /// `defs` should be present (i.e. not `None`) if and only if the
    /// constructed schema is a root one. This invariant is not enforced, but
    /// many users of this crate will presume that root schemas have definitions
    /// they can unwrap. Likewise, some tooling will assume that any schema
    /// which has non-`None` definitions are root schemas.
    pub fn from_parts(
        defs: Option<HashMap<String, Schema>>,
        form: Box<Form>,
        extra: HashMap<String, Value>,
    ) -> Schema {
        Schema { defs, form, extra }
    }

    /// Construct a new, root schema from a `Serde`.
    pub fn from_serde(serde_schema: Serde) -> Result<Self, Error> {
        let schema = Self::_from_serde(serde_schema, true)?;

        Self::check_refs(&schema.defs.as_ref().unwrap(), &schema)?;
        for sub_schema in schema.defs.as_ref().unwrap().values() {
            Self::check_refs(&schema.defs.as_ref().unwrap(), &sub_schema)?;
        }

        Ok(schema)
    }

    fn _from_serde(serde_schema: Serde, is_root: bool) -> Result<Self, Error> {
        let defs = if is_root {
            let mut defs = HashMap::new();
            for (name, sub_schema) in serde_schema.defs.unwrap_or_default() {
                defs.insert(name, Self::_from_serde(sub_schema, false)?);
            }
            Some(defs)
        } else {
            if serde_schema.defs.is_some() {
                bail!(JddfError::InvalidForm);
            } else {
                None
            }
        };

        let mut form = Form::Empty;

        if let Some(rxf) = serde_schema.rxf {
            form = Form::Ref(rxf);
        }

        if let Some(typ) = serde_schema.typ {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            form = Form::Type(match typ.as_ref() {
                "boolean" => Type::Boolean,
                "float32" => Type::Float32,
                "float64" => Type::Float64,
                "int8" => Type::Int8,
                "uint8" => Type::Uint8,
                "int16" => Type::Int16,
                "uint16" => Type::Uint16,
                "int32" => Type::Int32,
                "uint32" => Type::Uint32,
                "string" => Type::String,
                "timestamp" => Type::Timestamp,
                _ => bail!(JddfError::InvalidForm),
            });
        }

        if let Some(enm) = serde_schema.enm {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            let mut values = HashSet::new();
            for val in enm {
                if values.contains(&val) {
                    bail!(JddfError::InvalidForm);
                } else {
                    values.insert(val);
                }
            }

            if values.is_empty() {
                bail!(JddfError::InvalidForm);
            }

            form = Form::Enum(values);
        }

        if let Some(elements) = serde_schema.elems {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            form = Form::Elements(Self::_from_serde(*elements, false)?);
        }

        if serde_schema.props.is_some() || serde_schema.opt_props.is_some() {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            let allow_additional = serde_schema.additional_props == Some(true);
            let has_required = serde_schema.props.is_some();

            let mut required = HashMap::new();
            for (name, sub_schema) in serde_schema.props.unwrap_or_default() {
                required.insert(name, Self::_from_serde(sub_schema, false)?);
            }

            let mut optional = HashMap::new();
            for (name, sub_schema) in serde_schema.opt_props.unwrap_or_default() {
                if required.contains_key(&name) {
                    bail!(JddfError::AmbiguousProperty { property: name });
                }

                optional.insert(name, Self::_from_serde(sub_schema, false)?);
            }

            form = Form::Properties {
                required,
                optional,
                has_required,
                allow_additional,
            };
        }

        if let Some(values) = serde_schema.values {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            form = Form::Values(Self::_from_serde(*values, false)?);
        }

        if let Some(discriminator) = serde_schema.discriminator {
            if form != Form::Empty {
                bail!(JddfError::InvalidForm);
            }

            let mut mapping = HashMap::new();
            for (name, sub_schema) in discriminator.mapping {
                let sub_schema = Self::_from_serde(sub_schema, false)?;
                match sub_schema.form.as_ref() {
                    Form::Properties {
                        required, optional, ..
                    } => {
                        if required.contains_key(&discriminator.tag)
                            || optional.contains_key(&discriminator.tag)
                        {
                            bail!(JddfError::AmbiguousProperty {
                                property: discriminator.tag,
                            });
                        }
                    }
                    _ => bail!(JddfError::InvalidForm),
                };

                mapping.insert(name, sub_schema);
            }

            form = Form::Discriminator(discriminator.tag, mapping);
        }

        Ok(Self {
            defs,
            form: Box::new(form),
            extra: serde_schema.extra,
        })
    }

    fn check_refs(defs: &HashMap<String, Schema>, schema: &Schema) -> Result<(), Error> {
        match schema.form() {
            Form::Ref(ref def) => {
                if !defs.contains_key(def) {
                    bail!(JddfError::NoSuchDefinition {
                        definition: def.clone()
                    })
                }
            }
            Form::Elements(ref schema) => {
                Self::check_refs(defs, schema)?;
            }
            Form::Properties {
                ref required,
                ref optional,
                ..
            } => {
                for schema in required.values() {
                    Self::check_refs(defs, schema)?;
                }

                for schema in optional.values() {
                    Self::check_refs(defs, schema)?;
                }
            }
            Form::Values(ref schema) => {
                Self::check_refs(defs, schema)?;
            }
            Form::Discriminator(_, ref mapping) => {
                for schema in mapping.values() {
                    Self::check_refs(defs, schema)?;
                }
            }
            _ => {}
        };

        Ok(())
    }

    /// Convert this schema into a `Serde`.
    pub fn into_serde(self) -> Serde {
        let mut out = Serde::default();

        if let Some(defs) = self.defs {
            let mut out_defs = HashMap::new();
            for (name, value) in defs {
                out_defs.insert(name, value.into_serde());
            }

            out.defs = Some(out_defs);
        }

        match *self.form {
            Form::Empty => {}
            Form::Ref(def) => {
                out.rxf = Some(def);
            }
            Form::Type(Type::Boolean) => {
                out.typ = Some("boolean".to_owned());
            }
            Form::Type(Type::Float32) => {
                out.typ = Some("float32".to_owned());
            }
            Form::Type(Type::Float64) => {
                out.typ = Some("float64".to_owned());
            }
            Form::Type(Type::Int8) => {
                out.typ = Some("int8".to_owned());
            }
            Form::Type(Type::Uint8) => {
                out.typ = Some("uint8".to_owned());
            }
            Form::Type(Type::Int16) => {
                out.typ = Some("int16".to_owned());
            }
            Form::Type(Type::Uint16) => {
                out.typ = Some("uint16".to_owned());
            }
            Form::Type(Type::Int32) => {
                out.typ = Some("int32".to_owned());
            }
            Form::Type(Type::Uint32) => {
                out.typ = Some("uint32".to_owned());
            }
            Form::Type(Type::String) => {
                out.typ = Some("string".to_owned());
            }
            Form::Type(Type::Timestamp) => {
                out.typ = Some("timestamp".to_owned());
            }
            Form::Enum(vals) => {
                out.enm = Some(vals.into_iter().collect());
            }
            Form::Elements(sub_schema) => out.elems = Some(Box::new(sub_schema.into_serde())),
            Form::Properties {
                required,
                optional,
                has_required,
                ..
            } => {
                if has_required || !required.is_empty() {
                    out.props = Some(
                        required
                            .into_iter()
                            .map(|(k, v)| (k, v.into_serde()))
                            .collect(),
                    );
                }

                if !has_required || !optional.is_empty() {
                    out.opt_props = Some(
                        optional
                            .into_iter()
                            .map(|(k, v)| (k, v.into_serde()))
                            .collect(),
                    );
                }
            }
            Form::Values(sub_schema) => out.values = Some(Box::new(sub_schema.into_serde())),
            Form::Discriminator(tag, mapping) => {
                out.discriminator = Some(SerdeDiscriminator {
                    tag,
                    mapping: mapping
                        .into_iter()
                        .map(|(k, v)| (k, v.into_serde()))
                        .collect(),
                });
            }
        }

        out.extra = self.extra;
        out
    }

    /// Is this schema a root schema?
    ///
    /// Under the hood, this is entirely equivalent to checking whether
    /// `definitions().is_some()`.
    pub fn is_root(&self) -> bool {
        self.defs.is_some()
    }

    /// Get the definitions associated with this schema.
    ///
    /// If this schema is non-root, this returns None.
    pub fn definitions(&self) -> &Option<HashMap<String, Schema>> {
        &self.defs
    }

    /// Get the form of the schema.
    pub fn form(&self) -> &Form {
        &self.form
    }

    /// Get extra data associated with this schema.
    ///
    /// Essentially, this function returns a JSON object of properties that
    /// aren't JDDF keywords, but which were included in the schema's JSON. You
    /// might use these nonstandard fields to implement custom behavior.
    pub fn extra(&self) -> &HashMap<String, Value> {
        &self.extra
    }
}

/// The various forms which a schema may take on, and their respective data.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    /// The empty form.
    ///
    /// This schema accepts all data.
    Empty,

    /// The ref form.
    ///
    /// This schema refers to another schema, and does whatever that other
    /// schema does. The contained string is the name of the definition of the
    /// referred-to schema -- it is an index into the `defs` of the root schema.
    Ref(String),

    /// The type form.
    ///
    /// This schema asserts that the data is one of the primitive types.
    Type(Type),

    /// The enum form.
    ///
    /// This schema asserts that the data is a string, and that it is one of a
    /// set of values.
    Enum(HashSet<String>),

    /// The elements form.
    ///
    /// This schema asserts that the instance is an array, and that every
    /// element of the array matches a given schema.
    Elements(Schema),

    /// The properties form.
    ///
    /// This schema asserts that the instance is an object, and that the
    /// properties all satisfy their respective schemas.
    ///
    /// `required` is the set of required properties and their schemas.
    /// `optional` is the set of optional properties and their schemas.
    /// `allow_additional` indicates whether it's acceptable for the instance to
    /// have properties other than those mentioned in `required` and `optional`.
    ///
    /// `has_required` indicates whether `properties` exists on the schema. This
    /// allows implementations to distinguish the case of an empty `properties`
    /// field from an omitted one. This is necessary for tooling which wants to
    /// link to a particular part of a schema in JSON form.
    Properties {
        required: HashMap<String, Schema>,
        optional: HashMap<String, Schema>,
        allow_additional: bool,
        has_required: bool,
    },

    /// The values form.
    ///
    /// This schema asserts that the instance is an object, and that all the
    /// values in the object all satisfy the same schema.
    Values(Schema),

    /// The discriminator form.
    ///
    /// This schema asserts that the instance is an object, and that it has a
    /// "tag" property. The value of that tag must be one of the expected
    /// "mapping" keys, and the corresponding mapping value is a schema that the
    /// instance is expected to satisfy.
    ///
    /// The first parameter is the name of the tag property. The second
    /// parameter is the mapping from tag values to their corresponding schemas.
    Discriminator(String, HashMap<String, Schema>),
}

/// The values that the "type" keyword may check for.
///
/// In a certain sense, you can consider these types to be JSON's "primitive"
/// types, with the remaining two types, arrays and objects, being the "complex"
/// types covered by other keywords.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// The "true" or "false" JSON values.
    Boolean,

    /// A floating-point number. Signals the intention that the data is meant to
    /// be a single-precision float.
    Float32,

    /// A floating-point number. Signals the intention that the data is meant to
    /// be a double-precision float.
    Float64,

    /// An integer in the range covered by `i8`.
    Int8,

    /// An integer in the range covered by `u8`.
    Uint8,

    /// An integer in the range covered by `i16`.
    Int16,

    /// An integer in the range covered by `u16`.
    Uint16,

    /// An integer in the range covered by `i32`.
    Int32,

    /// An integer in the range covered by `u32`.
    Uint32,

    /// Any JSON string.
    String,

    /// A string encoding an RFC3339 timestamp.
    Timestamp,
}

/// A serialization/deserialization-friendly representation of a JDDF schema.
///
/// This struct is meant for use with the `serde` crate. It is excellent for
/// parsing from various data formats, but does not enforce all the semantic
/// rules about how schemas must be formed. For that, consider converting
/// instances of `Serde` into [`Schema`](struct.Schema.html) using
/// [`Schema::from_serde`](struct.Schema.html#method.from_serde).
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct Serde {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "definitions")]
    pub defs: Option<HashMap<String, Serde>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "additionalProperties")]
    pub additional_props: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ref")]
    pub rxf: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub typ: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "enum")]
    pub enm: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "elements")]
    pub elems: Option<Box<Serde>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "properties")]
    pub props: Option<HashMap<String, Serde>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "optionalProperties")]
    pub opt_props: Option<HashMap<String, Serde>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Box<Serde>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<SerdeDiscriminator>,

    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// A serialization/deserialization-friendly representation of a JDDF
/// discriminator.
///
/// This struct is useful mostly in the context of
/// [`SerdeSchema`](struct.SerdeSchema.html).
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct SerdeDiscriminator {
    #[serde(rename = "tag")]
    pub tag: String,
    pub mapping: HashMap<String, Serde>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_json() {
        let data = r#"{
  "definitions": {
    "a": {}
  },
  "additionalProperties": true,
  "ref": "http://example.com/bar",
  "type": "foo",
  "enum": [
    "FOO",
    "BAR"
  ],
  "elements": {},
  "properties": {
    "a": {}
  },
  "optionalProperties": {
    "a": {}
  },
  "values": {},
  "discriminator": {
    "tag": "foo",
    "mapping": {
      "a": {}
    }
  },
  "extra": "foo"
}"#;

        let parsed: Serde = serde_json::from_str(data).expect("failed to parse json");
        assert_eq!(
            parsed,
            Serde {
                rxf: Some("http://example.com/bar".to_owned()),
                defs: Some(
                    [("a".to_owned(), Serde::default())]
                        .iter()
                        .cloned()
                        .collect()
                ),
                additional_props: Some(true),
                typ: Some("foo".to_owned()),
                enm: Some(vec!["FOO".to_owned(), "BAR".to_owned()]),
                elems: Some(Box::new(Serde::default())),
                props: Some(
                    [("a".to_owned(), Serde::default())]
                        .iter()
                        .cloned()
                        .collect()
                ),
                opt_props: Some(
                    [("a".to_owned(), Serde::default())]
                        .iter()
                        .cloned()
                        .collect()
                ),
                values: Some(Box::new(Serde::default())),
                discriminator: Some(SerdeDiscriminator {
                    tag: "foo".to_owned(),
                    mapping: [("a".to_owned(), Serde::default())]
                        .iter()
                        .cloned()
                        .collect(),
                }),
                extra: [("extra".to_owned(), json!("foo"))]
                    .iter()
                    .cloned()
                    .collect(),
            }
        );

        let round_trip = serde_json::to_string_pretty(&parsed).expect("failed to serialize json");
        assert_eq!(round_trip, data);
    }

    #[test]
    fn from_serde_root() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "definitions": {
                        "a": { "type": "boolean" }
                    }
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(
                    [(
                        "a".to_owned(),
                        Schema {
                            defs: None,
                            form: Box::new(Form::Type(Type::Boolean)),
                            extra: HashMap::new(),
                        },
                    )]
                    .iter()
                    .cloned()
                    .collect()
                ),
                form: Box::new(Form::Empty),
                extra: HashMap::new(),
            }
        );
    }

    #[test]
    fn from_serde_empty() {
        assert_eq!(
            Schema::from_serde(serde_json::from_value(json!({})).unwrap()).unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Empty),
                extra: HashMap::new(),
            }
        );
    }

    #[test]
    fn from_serde_extra() {
        assert_eq!(
            Schema::from_serde(serde_json::from_value(json!({ "foo": "bar" })).unwrap()).unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Empty),
                extra: serde_json::from_value(json!({ "foo": "bar" })).unwrap(),
            }
        );
    }

    #[test]
    fn from_serde_ref() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "definitions": {
                        "a": { "type": "boolean" }
                    },
                    "ref": "a",
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(
                    [(
                        "a".to_owned(),
                        Schema {
                            defs: None,
                            form: Box::new(Form::Type(Type::Boolean)),
                            extra: HashMap::new(),
                        },
                    )]
                    .iter()
                    .cloned()
                    .collect()
                ),
                form: Box::new(Form::Ref("a".to_owned())),
                extra: HashMap::new(),
            }
        );

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "definitions": {
                    "a": { "type": "boolean" }
                },
                "ref": "",
            }))
            .unwrap()
        )
        .is_err());
    }

    #[test]
    fn from_serde_type() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "type": "boolean",
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Type(Type::Boolean)),
                extra: HashMap::new(),
            },
        );

        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "type": "float64",
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Type(Type::Float64)),
                extra: HashMap::new(),
            },
        );

        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "type": "string",
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Type(Type::String)),
                extra: HashMap::new(),
            },
        );

        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "type": "timestamp",
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Type(Type::Timestamp)),
                extra: HashMap::new(),
            },
        );

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "type": "nonsense",
            }))
            .unwrap()
        )
        .is_err());
    }

    #[test]
    fn from_serde_enum() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "enum": ["FOO", "BAR"],
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Enum(
                    vec!["FOO".to_owned(), "BAR".to_owned()]
                        .iter()
                        .cloned()
                        .collect()
                )),
                extra: HashMap::new(),
            },
        );

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "enum": [],
            }))
            .unwrap()
        )
        .is_err());

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "enum": ["FOO", "FOO"],
            }))
            .unwrap()
        )
        .is_err());
    }

    #[test]
    fn from_serde_elements() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "elements": {
                        "type": "boolean",
                    },
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Elements(Schema {
                    defs: None,
                    form: Box::new(Form::Type(Type::Boolean)),
                    extra: HashMap::new(),
                })),
                extra: HashMap::new(),
            }
        );
    }

    #[test]
    fn from_serde_properties() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "additionalProperties": true,
                    "properties": {
                        "a": { "type": "boolean" },
                    },
                    "optionalProperties": {
                        "b": { "type": "boolean" },
                    },
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Properties {
                    required: [(
                        "a".to_owned(),
                        Schema {
                            defs: None,
                            form: Box::new(Form::Type(Type::Boolean)),
                            extra: HashMap::new(),
                        }
                    )]
                    .iter()
                    .cloned()
                    .collect(),
                    optional: [(
                        "b".to_owned(),
                        Schema {
                            defs: None,
                            form: Box::new(Form::Type(Type::Boolean)),
                            extra: HashMap::new(),
                        }
                    )]
                    .iter()
                    .cloned()
                    .collect(),
                    has_required: true,
                    allow_additional: true,
                }),
                extra: HashMap::new(),
            }
        );

        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "optionalProperties": {
                        "b": { "type": "boolean" },
                    },
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Properties {
                    required: HashMap::new(),
                    optional: [(
                        "b".to_owned(),
                        Schema {
                            defs: None,
                            form: Box::new(Form::Type(Type::Boolean)),
                            extra: HashMap::new(),
                        }
                    )]
                    .iter()
                    .cloned()
                    .collect(),
                    has_required: false,
                    allow_additional: false,
                }),
                extra: HashMap::new(),
            }
        );

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "properties": {
                    "a": { "type": "boolean" },
                },
                "optionalProperties": {
                    "a": { "type": "boolean" },
                },
            }))
            .unwrap()
        )
        .is_err());
    }

    #[test]
    fn from_serde_values() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "values": {
                        "type": "boolean",
                    },
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Values(Schema {
                    defs: None,
                    form: Box::new(Form::Type(Type::Boolean)),
                    extra: HashMap::new(),
                })),
                extra: HashMap::new(),
            }
        );
    }

    #[test]
    fn from_serde_discriminator() {
        assert_eq!(
            Schema::from_serde(
                serde_json::from_value(json!({
                    "discriminator": {
                        "tag": "foo",
                        "mapping": {
                            "a": { "properties": {} },
                            "b": { "properties": {} },
                        },
                    },
                }))
                .unwrap()
            )
            .unwrap(),
            Schema {
                defs: Some(HashMap::new()),
                form: Box::new(Form::Discriminator(
                    "foo".to_owned(),
                    [
                        (
                            "a".to_owned(),
                            Schema {
                                defs: None,
                                form: Box::new(Form::Properties {
                                    required: HashMap::new(),
                                    optional: HashMap::new(),
                                    has_required: true,
                                    allow_additional: false,
                                }),
                                extra: HashMap::new(),
                            }
                        ),
                        (
                            "b".to_owned(),
                            Schema {
                                defs: None,
                                form: Box::new(Form::Properties {
                                    required: HashMap::new(),
                                    optional: HashMap::new(),
                                    has_required: true,
                                    allow_additional: false,
                                }),
                                extra: HashMap::new(),
                            }
                        )
                    ]
                    .iter()
                    .cloned()
                    .collect(),
                )),
                extra: HashMap::new(),
            }
        );

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "discriminator": {
                    "tag": "foo",
                    "mapping": {
                        "a": { "type": "boolean" },
                    }
                },
            }))
            .unwrap()
        )
        .is_err());

        assert!(Schema::from_serde(
            serde_json::from_value(json!({
                "discriminator": {
                    "tag": "foo",
                    "mapping": {
                        "a": {
                            "properties": {
                                "foo": { "type": "boolean" },
                            },
                        },
                    },
                },
            }))
            .unwrap()
        )
        .is_err());
    }
}
