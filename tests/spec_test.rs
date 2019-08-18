use jddf::{Config, Schema, SerdeSchema, Validator};
use serde::Deserialize;
use serde_json::Value;
use std::fs;

#[test]
fn spec_invalid_schemas() -> Result<(), std::io::Error> {
    #[derive(Deserialize)]
    struct TestCase {
        name: String,
        schema: Value,
    }

    let test_cases: Vec<TestCase> =
        serde_json::from_slice(&fs::read("spec/tests/invalid-schemas.json")?)
            .expect("error parsing invalid-schemas.json");
    for test_case in test_cases {
        println!("{}", test_case.name);

        if let Ok(serde_schema) = serde_json::from_value::<SerdeSchema>(test_case.schema) {
            assert!(Schema::from_serde(serde_schema).is_err());
        }
    }

    Ok(())
}

#[test]
fn spec_validation() -> Result<(), std::io::Error> {
    #[derive(Deserialize)]
    struct TestSuite {
        name: String,
        schema: SerdeSchema,
        instances: Vec<TestCase>,
    }

    #[derive(Deserialize)]
    struct TestCase {
        instance: Value,
        errors: Vec<TestCaseError>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestCaseError {
        #[serde(rename = "instancePath")]
        instance_path: String,

        #[serde(rename = "schemaPath")]
        schema_path: String,
    }

    let mut test_files: Vec<_> = fs::read_dir("spec/tests/validation")?
        .map(|entry| entry.expect("error getting dir entry").path())
        .collect();
    test_files.sort();

    for path in test_files {
        println!("{:?}", &path);
        let file = fs::read(path)?;
        let suites: Vec<TestSuite> = serde_json::from_slice(&file)?;

        for (i, suite) in suites.into_iter().enumerate() {
            println!("{}: {}", i, suite.name);

            let schema = Schema::from_serde(suite.schema).expect("error parsing schema");

            let validator = Validator::new_with_config(Config::new());

            for (j, mut test_case) in suite.instances.into_iter().enumerate() {
                println!("{}/{}", i, j);

                let mut actual_errors: Vec<_> = validator
                    .validate(&schema, &test_case.instance)
                    .expect("error validating instance")
                    .into_iter()
                    .map(|error| TestCaseError {
                        instance_path: error.instance_path().to_string(),
                        schema_path: error.schema_path().to_string(),
                    })
                    .collect();

                actual_errors
                    .sort_by_key(|err| format!("{},{}", err.schema_path, err.instance_path));
                test_case
                    .errors
                    .sort_by_key(|err| format!("{},{}", err.schema_path, err.instance_path));

                assert_eq!(actual_errors, test_case.errors);
            }
        }
    }

    Ok(())
}
