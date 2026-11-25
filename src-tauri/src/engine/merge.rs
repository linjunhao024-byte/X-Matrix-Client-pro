//! The profile's YAML override, applied to a rendered kernel config.
//!
//! Why an overlay and not a template: the renderers already know how to spell
//! every field each kernel wants, and re-deriving a whole config from user text
//! would mean tracking two kernels' schemas forever. The overlay only has to
//! say what changes — "here is the DNS block I want" — and this module puts it
//! where it belongs, after rendering and before the file is written.
//!
//! Why YAML and not JavaScript: Clash Verge's `script` mode needs a JS engine
//! (qjs/v8) plus a sandbox, which is a large dependency and a large risk for a
//! feature that mostly re-spells fields. A deep merge expresses the same
//! changes declaratively. **This project does not support script overrides**,
//! and the UI says so — merge is not script with another name.
//!
//! The rules are deliberately small enough to explain in one sentence each:
//!
//! * both sides are objects → recurse by key; the overlay wins on a collision;
//! * an array is replaced **whole** — never concatenated, never merged by index
//!   (appending to `servers` is how a DNS list ends up with two entries the
//!   user did not write);
//! * anything else (string, number, bool, null) → the overlay wins;
//! * an empty overlay leaves the config byte-for-byte untouched.
//!
//! `null` means "set this key to null", not "delete this key": deleting would
//! make the meaning of a key depend on the base config, and both kernels treat
//! an explicit null as "leave this alone" anyway.

use serde_json::Value;

use crate::error::{AppError, AppResult};

/// Merge `overlay` (YAML text) into `base` (a rendered kernel config).
///
/// An empty or whitespace-only overlay hands the base back exactly as it came
/// in — this is the path every profile without an override takes, so it must
/// not even re-serialise the document.
pub fn merge_text(base: &Value, overlay: &str) -> AppResult<Value> {
    if overlay.trim().is_empty() {
        return Ok(base.clone());
    }
    let parsed: serde_yaml::Value = serde_yaml::from_str(overlay)
        .map_err(|error| AppError::InvalidInput(format!("覆盖内容不是合法 YAML：{error}")))?;
    let overlay = yaml_to_json(parsed)?;
    if !overlay.is_object() {
        return Err(AppError::InvalidInput(format!(
            "覆盖内容的顶层必须是一个映射（key: value），现在是 {}",
            type_name(&overlay)
        )));
    }
    Ok(merge_value(base.clone(), overlay))
}

/// The "one sentence each" from the module docs, in code.
fn merge_value(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                let placed = match base.remove(&key) {
                    // Both objects: the child keys are what the user edited,
                    // the siblings they did not mention must survive.
                    Some(existing) => merge_value(existing, value),
                    // A key the base does not have: nothing to merge with.
                    None => value,
                };
                base.insert(key, placed);
            }
            Value::Object(base)
        }
        // Arrays land here on purpose: `(Array, Array)` is not a special case,
        // so the overlay replaces the whole list. Scalars do the same.
        (_, overlay) => overlay,
    }
}

/// Whatever the overlay is, spelled the way the refusal message wants it.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "映射",
    }
}

/// Convert a parsed YAML value into the JSON shape the renderers produce.
///
/// Not `serde_json::to_value(yaml)`: that fails on a non-string key with a
/// generic message, and silently loses the difference between "this is a YAML
/// tag" and "this is a plain value". Both cases are refused here with a
/// sentence that names what to change, because the kernels read JSON and
/// neither of them has a tag or a numeric key.
fn yaml_to_json(value: serde_yaml::Value) -> AppResult<Value> {
    use serde_yaml::Value as Yaml;
    Ok(match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(flag) => Value::Bool(flag),
        Yaml::Number(number) => {
            if let Some(int) = number.as_i64() {
                Value::Number(int.into())
            } else if let Some(uint) = number.as_u64() {
                Value::Number(uint.into())
            } else {
                let float = number.as_f64().unwrap_or(0.0);
                match serde_json::Number::from_f64(float) {
                    Some(number) => Value::Number(number),
                    None => {
                        return Err(AppError::InvalidInput(format!(
                            "覆盖内容里的数字 {float} 不是 JSON 能表示的数（NaN 和无穷大不行）"
                        )))
                    }
                }
            }
        }
        Yaml::String(text) => Value::String(text),
        Yaml::Sequence(items) => {
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                list.push(yaml_to_json(item)?);
            }
            Value::Array(list)
        }
        Yaml::Mapping(map) => {
            let mut object = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                let key = key.as_str().ok_or_else(|| {
                    AppError::InvalidInput(format!(
                        "覆盖内容的键必须是文本：YAML 允许数字或布尔键，内核配置的 JSON 不允许（{key:?}）"
                    ))
                })?;
                object.insert(key.to_string(), yaml_to_json(item)?);
            }
            Value::Object(object)
        }
        Yaml::Tagged(tagged) => {
            return Err(AppError::InvalidInput(format!(
                "覆盖内容里出现了 YAML 标记（{}）：内核配置里没有这种写法",
                tagged.tag
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_empty_overlay_returns_the_base_byte_for_byte() {
        let base = json!({
            "log": {"loglevel": "warning"},
            "outbounds": [{"tag": "proxy"}, {"tag": "direct"}],
        });
        let rendered = serde_json::to_string(&base).unwrap();

        // Every spelling of "the user typed nothing" must be a no-op, and the
        // result must serialise to the same bytes — not merely compare equal.
        for empty in ["", " ", "\n", "  \r\n\t "] {
            let merged = merge_text(&base, empty).unwrap();
            assert_eq!(merged, base, "{empty:?}");
            assert_eq!(serde_json::to_string(&merged).unwrap(), rendered, "{empty:?}");
        }
    }

    #[test]
    fn nested_objects_merge_key_by_key() {
        let base = json!({
            "log": {"loglevel": "warning", "access": ""},
            "inbounds": [{"port": 7890}],
        });
        let merged = merge_text(&base, "log:\n  loglevel: debug\n").unwrap();
        assert_eq!(merged["log"]["loglevel"], json!("debug"));
        // The sibling the overlay did not name survives: an override is an
        // amendment, not a replacement of the whole object.
        assert_eq!(merged["log"]["access"], json!(""));
        assert_eq!(merged["inbounds"][0]["port"], json!(7890));
    }

    #[test]
    fn an_array_is_replaced_whole_not_concatenated() {
        let base = json!({"dns": {"servers": ["1.1.1.1", "8.8.8.8", "9.9.9.9"]}});
        let merged = merge_text(&base, "dns:\n  servers:\n    - 223.5.5.5\n").unwrap();
        let servers = merged["dns"]["servers"].as_array().expect("still an array");
        assert_eq!(servers.len(), 1, "a three-element base and a one-element overlay leave one");
        assert_eq!(servers[0], json!("223.5.5.5"));
    }

    #[test]
    fn scalars_are_overwritten_and_null_can_be_written() {
        let base = json!({"a": "text", "b": 1, "c": false, "d": {"keep": true}});
        let merged = merge_text(&base, "a: 42\nb: true\nc: null\nd: null\n").unwrap();
        assert_eq!(merged["a"], json!(42));
        assert_eq!(merged["b"], json!(true));
        // null written on purpose: the key stays, its value is null.
        assert_eq!(merged["c"], Value::Null);
        assert!(merged.get("d").is_some(), "the key stays, the object is replaced");
        assert_eq!(merged["d"], Value::Null);
    }

    #[test]
    fn a_top_level_array_or_scalar_is_refused() {
        for (text, shown) in [("- a\n- b\n", "数组"), ("just a string", "字符串"), ("42", "数字")] {
            let error = merge_text(&json!({}), text).unwrap_err();
            let message = error.to_string();
            assert!(
                message.starts_with("覆盖内容的顶层必须是一个映射（key: value），现在是 "),
                "{message}"
            );
            assert!(message.ends_with(shown), "{message} must name {shown}");
        }
    }

    #[test]
    fn broken_yaml_is_refused_with_the_parser_message() {
        let error = merge_text(&json!({}), "dns: [unclosed\n").unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message.starts_with("覆盖内容不是合法 YAML：")),
            "{error:?}"
        );
    }

    #[test]
    fn a_non_string_key_is_refused_instead_of_crashing() {
        // YAML allows `1: value`; JSON does not. Refusing with a sentence beats
        // letting serde_json fail with "key must be a string".
        let error = merge_text(&json!({}), "1: value\n").unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message.starts_with("覆盖内容的键必须是文本：")),
            "{error:?}"
        );
    }

    #[test]
    // serde_yaml resolves the standard tags (!!binary and friends) on its own, so
    // the only tag that reaches this branch is a custom one like !custom.
    fn a_yaml_tag_is_refused() {
        let error = merge_text(&json!({}), "dns: !custom hi\n").unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message.starts_with("覆盖内容里出现了 YAML 标记")),
            "{error:?}"
        );
    }
}