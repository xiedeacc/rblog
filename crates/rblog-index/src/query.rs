//! Selectors and query execution against [`IndexedExt`].

use std::cmp::Ordering;

use crate::IndexedExt;

/// Dotted JSON path, e.g. `spec.publishTime`.
///
/// Path tokens that are valid integers index into JSON arrays; everything else
/// indexes into an object. Missing intermediates resolve to `Null`.
pub type JsonPath = String;

/// Label-selector grammar, matching Kubernetes set-based label requirements.
#[derive(Debug, Clone)]
pub enum LabelSelector {
    Equals { key: String, value: String },
    NotEquals { key: String, value: String },
    In { key: String, values: Vec<String> },
    NotIn { key: String, values: Vec<String> },
    Exists(String),
    NotExists(String),
}

impl LabelSelector {
    fn key(&self) -> &str {
        match self {
            LabelSelector::Equals { key, .. }
            | LabelSelector::NotEquals { key, .. }
            | LabelSelector::In { key, .. }
            | LabelSelector::NotIn { key, .. }
            | LabelSelector::Exists(key)
            | LabelSelector::NotExists(key) => key,
        }
    }

    fn matches(&self, e: &IndexedExt) -> bool {
        let value = e.labels.get(self.key()).map(String::as_str);
        match self {
            LabelSelector::Equals { value: v, .. } => value == Some(v.as_str()),
            LabelSelector::NotEquals { value: v, .. } => value != Some(v.as_str()),
            LabelSelector::In { values, .. } => {
                value.is_some_and(|cur| values.iter().any(|v| v == cur))
            }
            LabelSelector::NotIn { values, .. } => {
                value.is_none_or(|cur| !values.iter().any(|v| v == cur))
            }
            LabelSelector::Exists(_) => value.is_some(),
            LabelSelector::NotExists(_) => value.is_none(),
        }
    }
}

/// Field-selector. Path is dot-notation, value is compared via JSON equality.
#[derive(Debug, Clone)]
pub enum FieldSelector {
    Equals {
        path: JsonPath,
        value: serde_json::Value,
    },
    NotEquals {
        path: JsonPath,
        value: serde_json::Value,
    },
}

impl FieldSelector {
    fn matches(&self, e: &IndexedExt) -> bool {
        let actual = resolve_path(&e.raw, self.path());
        match self {
            FieldSelector::Equals { value, .. } => json_eq(actual, value),
            FieldSelector::NotEquals { value, .. } => !json_eq(actual, value),
        }
    }

    fn path(&self) -> &str {
        match self {
            FieldSelector::Equals { path, .. } | FieldSelector::NotEquals { path, .. } => path,
        }
    }
}

fn json_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    // Numeric coercion: 3 == 3.0 == "3" is NOT true; we follow JSON's strict
    // equality, which is what Halo's IndexEngine.equal also does.
    a == b
}

/// Walks `value` following `path`. Tokens that parse as `usize` index into
/// arrays. Missing path -> `Null`.
pub(crate) fn resolve_path<'a>(value: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    static NULL: serde_json::Value = serde_json::Value::Null;
    let mut cur = value;
    for token in path.split('.') {
        if token.is_empty() {
            continue;
        }
        cur = if let Ok(idx) = token.parse::<usize>() {
            cur.get(idx).unwrap_or(&NULL)
        } else {
            cur.get(token).unwrap_or(&NULL)
        };
    }
    cur
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub path: JsonPath,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ListOptions {
    pub label_selectors: Vec<LabelSelector>,
    pub field_selectors: Vec<FieldSelector>,
    pub sort: Option<Sort>,
    pub page: Option<Page>,
}

impl ListOptions {
    /// Builder: append a label selector and return self.
    #[must_use]
    pub fn with_label(mut self, selector: LabelSelector) -> Self {
        self.label_selectors.push(selector);
        self
    }

    #[must_use]
    pub fn with_field(mut self, selector: FieldSelector) -> Self {
        self.field_selectors.push(selector);
        self
    }

    #[must_use]
    pub fn sorted_by(mut self, path: impl Into<String>, direction: SortDirection) -> Self {
        self.sort = Some(Sort {
            path: path.into(),
            direction,
        });
        self
    }

    #[must_use]
    pub fn paged(mut self, offset: usize, limit: usize) -> Self {
        self.page = Some(Page { offset, limit });
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct ListResult {
    pub items: Vec<IndexedExt>,
    pub total: usize,
}

pub(crate) fn matches(e: &IndexedExt, opts: &ListOptions) -> bool {
    opts.label_selectors.iter().all(|s| s.matches(e))
        && opts.field_selectors.iter().all(|s| s.matches(e))
}

pub(crate) fn sort_in_place(items: &mut [&IndexedExt], sort: &Sort) {
    items.sort_by(|a, b| {
        let av = resolve_path(&a.raw, &sort.path);
        let bv = resolve_path(&b.raw, &sort.path);
        let ord = compare_json(av, bv);
        match sort.direction {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        }
    });
}

/// Order JSON values across the types that matter for sort keys:
///
/// - numbers: numeric ordering;
/// - strings: lexicographic; if both parse as RFC3339, compare as timestamps;
/// - booleans: false < true;
/// - everything else (including `Null`): equal, falling back to name order at
///   the caller's discretion.
fn compare_json(a: &serde_json::Value, b: &serde_json::Value) -> Ordering {
    use serde_json::Value;
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Number(x), Value::Number(y)) => {
            let xf = x.as_f64().unwrap_or(0.0);
            let yf = y.as_f64().unwrap_or(0.0);
            xf.partial_cmp(&yf).unwrap_or(Ordering::Equal)
        }
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::String(x), Value::String(y)) => {
            if let (Ok(xi), Ok(yi)) = (
                chrono::DateTime::parse_from_rfc3339(x),
                chrono::DateTime::parse_from_rfc3339(y),
            ) {
                xi.cmp(&yi)
            } else {
                x.cmp(y)
            }
        }
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_post(name: &str, published: bool, priority: i32, title: &str) -> IndexedExt {
        let v = serde_json::json!({
            "metadata": {
                "name": name,
                "labels": {
                    "content.halo.run/published": if published { "true" } else { "false" },
                }
            },
            "spec": { "title": title, "priority": priority }
        });
        IndexedExt::from_value(v).unwrap()
    }

    #[test]
    fn equality_label_selector() {
        let p1 = make_post("a", true, 0, "A");
        let p2 = make_post("b", false, 0, "B");
        let sel = LabelSelector::Equals {
            key: "content.halo.run/published".to_owned(),
            value: "true".to_owned(),
        };
        assert!(sel.matches(&p1));
        assert!(!sel.matches(&p2));
    }

    #[test]
    fn in_and_not_in_label_selectors() {
        let p1 = make_post("a", true, 0, "A");
        let in_sel = LabelSelector::In {
            key: "content.halo.run/published".to_owned(),
            values: vec!["true".to_owned(), "maybe".to_owned()],
        };
        let notin_sel = LabelSelector::NotIn {
            key: "content.halo.run/published".to_owned(),
            values: vec!["false".to_owned()],
        };
        assert!(in_sel.matches(&p1));
        assert!(notin_sel.matches(&p1));
    }

    #[test]
    fn field_selector_equals_on_spec_field() {
        let p = make_post("a", true, 7, "Hello");
        let s = FieldSelector::Equals {
            path: "spec.title".to_owned(),
            value: serde_json::Value::String("Hello".to_owned()),
        };
        assert!(s.matches(&p));

        let s2 = FieldSelector::NotEquals {
            path: "spec.priority".to_owned(),
            value: serde_json::json!(0),
        };
        assert!(s2.matches(&p));
    }

    #[test]
    fn sort_strings_lexicographic() {
        let mut items = [
            make_post("a", true, 0, "Zebra"),
            make_post("b", true, 0, "Apple"),
            make_post("c", true, 0, "Mango"),
        ];
        let mut refs: Vec<&IndexedExt> = items.iter_mut().map(|e| &*e).collect();
        let sort = Sort {
            path: "spec.title".to_owned(),
            direction: SortDirection::Asc,
        };
        sort_in_place(&mut refs, &sort);
        assert_eq!(
            refs.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
    }

    #[test]
    fn sort_numbers_desc() {
        let items = [
            make_post("a", true, 5, ""),
            make_post("b", true, 1, ""),
            make_post("c", true, 9, ""),
        ];
        let mut refs: Vec<&IndexedExt> = items.iter().collect();
        let sort = Sort {
            path: "spec.priority".to_owned(),
            direction: SortDirection::Desc,
        };
        sort_in_place(&mut refs, &sort);
        assert_eq!(
            refs.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
    }

    #[test]
    fn sort_strings_as_timestamps_when_rfc3339() {
        let earlier = serde_json::json!({
            "metadata": {"name": "early"},
            "spec": {"publishTime": "2026-01-01T00:00:00Z"}
        });
        let later = serde_json::json!({
            "metadata": {"name": "late"},
            "spec": {"publishTime": "2026-06-01T00:00:00Z"}
        });
        let items = [
            IndexedExt::from_value(later).unwrap(),
            IndexedExt::from_value(earlier).unwrap(),
        ];
        let mut refs: Vec<&IndexedExt> = items.iter().collect();
        sort_in_place(
            &mut refs,
            &Sort {
                path: "spec.publishTime".to_owned(),
                direction: SortDirection::Asc,
            },
        );
        assert_eq!(refs[0].name, "early");
        assert_eq!(refs[1].name, "late");
    }
}
