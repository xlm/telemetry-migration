use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;

pub fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct OptionalFields {
        #[serde(default, deserialize_with = "super::lenient")]
        float_value: Option<f64>,
        #[serde(default, deserialize_with = "super::lenient")]
        integer_value: Option<i64>,
    }

    #[test]
    fn missing_fields_become_none() {
        let fields: OptionalFields = serde_json::from_str("{}").expect("missing fields");

        assert_eq!(
            fields,
            OptionalFields {
                float_value: None,
                integer_value: None
            }
        );
    }

    #[test]
    fn null_fields_become_none() {
        let fields: OptionalFields =
            serde_json::from_str(r#"{"float_value":null,"integer_value":null}"#)
                .expect("null fields");

        assert_eq!(
            fields,
            OptionalFields {
                float_value: None,
                integer_value: None
            }
        );
    }

    #[test]
    fn wrong_typed_fields_become_none() {
        let fields: OptionalFields =
            serde_json::from_str(r#"{"float_value":"not a float","integer_value":1.5}"#)
                .expect("wrong typed fields");

        assert_eq!(
            fields,
            OptionalFields {
                float_value: None,
                integer_value: None
            }
        );
    }

    #[test]
    fn valid_fields_are_deserialized() {
        let fields: OptionalFields =
            serde_json::from_str(r#"{"float_value":12.5,"integer_value":9}"#)
                .expect("valid fields");

        assert_eq!(
            fields,
            OptionalFields {
                float_value: Some(12.5),
                integer_value: Some(9)
            }
        );
    }
}
