use serde_json::Value;

/// Extracts values from JSON based on a list of paths (e.g. "key/subkey").
/// Returns found values joined by space.
pub fn extract_message(value: &Value, paths: &[String]) -> Option<String> {
    let mut results = Vec::new();
    
    for path in paths {
        let keys: Vec<&str> = path.split('/').collect();
        if let Some(val) = extract_single_value(value, &keys) {
            results.push(val);
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results.join(" "))
    }
}

fn extract_single_value(value: &Value, keys: &[&str]) -> Option<String> {
    let mut current = value;
    for key in keys {
        if let Some(v) = current.get(*key) {
            current = v;
        } else if let Ok(idx) = key.parse::<usize>() {
            if let Some(v) = current.get(idx) {
                current = v;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    match current {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Object(_) | Value::Array(_) => Some(current.to_string()),
        Value::Null => None,
    }
}
