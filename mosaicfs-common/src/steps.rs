use chrono::{DateTime, Utc};

use crate::documents::{FileDocument, Step, StepResult};

/// Context provider for step evaluation — supplies label, access, replica, and annotation lookups.
pub trait StepContext {
    fn has_label(&self, file_uuid: &str, label: &str) -> bool;
    fn last_access(&self, file_id: &str) -> Option<DateTime<Utc>>;
    fn has_replica(&self, file_uuid: &str, target: Option<&str>, status: Option<&str>) -> bool;
    fn has_annotation(&self, file_uuid: &str, plugin_name: &str) -> bool;
}

/// Evaluate a sequence of steps against a file, returning Include or Exclude.
///
/// Steps are typically the concatenation of ancestor (inherited) steps followed by mount steps.
/// Each step is evaluated in order; on match the step's `on_match` (default Include) is checked:
/// - Include / Exclude → return immediately
/// - Continue → proceed to next step
/// After all steps, return `default_result`.
pub fn evaluate_steps(
    steps: &[Step],
    file: &FileDocument,
    file_id: &str,
    default_result: &StepResult,
    context: &dyn StepContext,
) -> StepResult {
    let file_uuid = crate::documents::FileDocument::uuid_from_id(file_id).unwrap_or(file_id);
    let now = Utc::now();

    for step in steps {
        let matched = evaluate_op(step, file, file_id, file_uuid, now, context);
        let effective = if step.invert { !matched } else { matched };

        if effective {
            let result = step.on_match.clone().unwrap_or(StepResult::Include);
            match result {
                StepResult::Continue => continue,
                other => return other,
            }
        }
    }

    default_result.clone()
}

fn evaluate_op(
    step: &Step,
    file: &FileDocument,
    file_id: &str,
    file_uuid: &str,
    now: DateTime<Utc>,
    context: &dyn StepContext,
) -> bool {
    match step.op.as_str() {
        "glob" => {
            let pattern = match step.params.get("pattern").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return false,
            };
            glob::Pattern::new(pattern)
                .map(|p| p.matches(&file.name))
                .unwrap_or(false)
        }
        "regex" => {
            let pattern = match step.params.get("pattern").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return false,
            };
            regex::Regex::new(pattern)
                .map(|r| r.is_match(&file.name))
                .unwrap_or(false)
        }
        "age" => {
            let days = match step.params.get("days").and_then(|v| v.as_i64()) {
                Some(d) => d,
                None => return false,
            };
            let comparison = step.params.get("comparison").and_then(|v| v.as_str()).unwrap_or("gt");
            let file_age = (now - file.mtime).num_days();
            compare_i64(file_age, days, comparison)
        }
        "size" => {
            let bytes = match step.params.get("bytes").and_then(|v| v.as_u64()) {
                Some(b) => b,
                None => return false,
            };
            let comparison = step.params.get("comparison").and_then(|v| v.as_str()).unwrap_or("gt");
            compare_u64(file.size, bytes, comparison)
        }
        "mime" => {
            let pattern = match step.params.get("pattern").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return false,
            };
            file.mime_type.as_deref().map(|m| m.contains(pattern)).unwrap_or(false)
        }
        "node" => {
            let node_id = match step.params.get("node_id").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => return false,
            };
            file.source.node_id == node_id
        }
        "label" => {
            let label = match step.params.get("label").and_then(|v| v.as_str()) {
                Some(l) => l,
                None => return false,
            };
            context.has_label(file_uuid, label)
        }
        "access_age" => {
            let days = match step.params.get("days").and_then(|v| v.as_i64()) {
                Some(d) => d,
                None => return false,
            };
            let comparison = step.params.get("comparison").and_then(|v| v.as_str()).unwrap_or("gt");
            let missing = step.params.get("missing").and_then(|v| v.as_str()).unwrap_or("include");

            match context.last_access(file_id) {
                Some(last) => {
                    let age = (now - last).num_days();
                    compare_i64(age, days, comparison)
                }
                None => missing == "include",
            }
        }
        "replicated" => {
            let target = step.params.get("target").and_then(|v| v.as_str());
            let status = step.params.get("status").and_then(|v| v.as_str());
            context.has_replica(file_uuid, target, status)
        }
        "annotation" => {
            let plugin_name = match step.params.get("plugin_name").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return false,
            };
            context.has_annotation(file_uuid, plugin_name)
        }
        _ => false,
    }
}

fn compare_i64(value: i64, threshold: i64, comparison: &str) -> bool {
    match comparison {
        "lt" => value < threshold,
        "gt" => value > threshold,
        "eq" => value == threshold,
        _ => false,
    }
}

fn compare_u64(value: u64, threshold: u64, comparison: &str) -> bool {
    match comparison {
        "lt" => value < threshold,
        "gt" => value > threshold,
        "eq" => value == threshold,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::*;
    use std::collections::{HashMap, HashSet};

    struct MockContext {
        labels: HashMap<String, HashSet<String>>,
        accesses: HashMap<String, DateTime<Utc>>,
        replicas: HashSet<String>, // "uuid:target:status"
        annotations: HashSet<String>, // "uuid:plugin"
    }

    impl MockContext {
        fn new() -> Self {
            Self {
                labels: HashMap::new(),
                accesses: HashMap::new(),
                replicas: HashSet::new(),
                annotations: HashSet::new(),
            }
        }
    }

    impl StepContext for MockContext {
        fn has_label(&self, file_uuid: &str, label: &str) -> bool {
            self.labels.get(file_uuid).map(|s| s.contains(label)).unwrap_or(false)
        }
        fn last_access(&self, file_id: &str) -> Option<DateTime<Utc>> {
            self.accesses.get(file_id).copied()
        }
        fn has_replica(&self, file_uuid: &str, target: Option<&str>, status: Option<&str>) -> bool {
            let key = format!("{}:{}:{}", file_uuid, target.unwrap_or("*"), status.unwrap_or("*"));
            if self.replicas.contains(&key) {
                return true;
            }
            // Check wildcard combinations
            for r in &self.replicas {
                let parts: Vec<&str> = r.splitn(3, ':').collect();
                if parts.len() == 3
                    && parts[0] == file_uuid
                    && (target.is_none() || parts[1] == target.unwrap())
                    && (status.is_none() || parts[2] == status.unwrap())
                {
                    return true;
                }
            }
            false
        }
        fn has_annotation(&self, file_uuid: &str, plugin_name: &str) -> bool {
            self.annotations.contains(&format!("{}:{}", file_uuid, plugin_name))
        }
    }

    fn test_file() -> FileDocument {
        FileDocument {
            doc_type: FileType::File,
            inode: 1,
            name: "report.pdf".to_string(),
            source: FileSource {
                node_id: "node-1".to_string(),
                export_path: "/docs/report.pdf".to_string(),
                export_parent: "/docs".to_string(),
            },
            size: 1_000_000,
            mtime: Utc::now() - chrono::Duration::days(30),
            mime_type: Some("application/pdf".to_string()),
            status: FileStatus::Active,
            deleted_at: None,
            migrated_from: None,
        }
    }

    fn make_step(op: &str, params: serde_json::Value) -> Step {
        let map = match params {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        Step {
            op: op.to_string(),
            invert: false,
            on_match: None,
            params: map,
        }
    }

    // --- glob ---

    #[test]
    fn test_glob_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("glob", serde_json::json!({"pattern": "*.pdf"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_glob_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("glob", serde_json::json!({"pattern": "*.txt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    #[test]
    fn test_glob_invert() {
        let file = test_file();
        let ctx = MockContext::new();
        let mut step = make_step("glob", serde_json::json!({"pattern": "*.pdf"}));
        step.invert = true;
        let result = evaluate_steps(&[step], &file, "file::abc", &StepResult::Include, &ctx);
        // pdf matches glob, invert makes it not-match, so falls through to default
        assert_eq!(result, StepResult::Include);
    }

    // --- regex ---

    #[test]
    fn test_regex_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("regex", serde_json::json!({"pattern": r"^report\.\w+$"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_regex_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("regex", serde_json::json!({"pattern": r"^notes\.\w+$"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- age ---

    #[test]
    fn test_age_gt() {
        let file = test_file(); // 30 days old
        let ctx = MockContext::new();
        let steps = vec![make_step("age", serde_json::json!({"days": 20, "comparison": "gt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_age_lt() {
        let file = test_file(); // 30 days old
        let ctx = MockContext::new();
        let steps = vec![make_step("age", serde_json::json!({"days": 20, "comparison": "lt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- size ---

    #[test]
    fn test_size_gt() {
        let file = test_file(); // 1_000_000
        let ctx = MockContext::new();
        let steps = vec![make_step("size", serde_json::json!({"bytes": 500_000, "comparison": "gt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_size_lt() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("size", serde_json::json!({"bytes": 500_000, "comparison": "lt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- mime ---

    #[test]
    fn test_mime_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("mime", serde_json::json!({"pattern": "pdf"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_mime_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("mime", serde_json::json!({"pattern": "image"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- node ---

    #[test]
    fn test_node_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("node", serde_json::json!({"node_id": "node-1"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_node_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("node", serde_json::json!({"node_id": "node-2"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- label ---

    #[test]
    fn test_label_match() {
        let file = test_file();
        let mut ctx = MockContext::new();
        ctx.labels.insert("abc".to_string(), HashSet::from(["important".to_string()]));
        let steps = vec![make_step("label", serde_json::json!({"label": "important"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_label_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("label", serde_json::json!({"label": "important"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- access_age ---

    #[test]
    fn test_access_age_match() {
        let file = test_file();
        let mut ctx = MockContext::new();
        ctx.accesses.insert("file::abc".to_string(), Utc::now() - chrono::Duration::days(60));
        let steps = vec![make_step("access_age", serde_json::json!({"days": 30, "comparison": "gt"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_access_age_missing_include() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("access_age", serde_json::json!({"days": 30, "comparison": "gt", "missing": "include"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_access_age_missing_exclude() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("access_age", serde_json::json!({"days": 30, "comparison": "gt", "missing": "exclude"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        // missing=exclude means no-match for never-accessed files, falls through to default
        assert_eq!(result, StepResult::Exclude);
    }

    // --- replicated ---

    #[test]
    fn test_replicated_match() {
        let file = test_file();
        let mut ctx = MockContext::new();
        ctx.replicas.insert("abc:offsite:current".to_string());
        let steps = vec![make_step("replicated", serde_json::json!({"target": "offsite", "status": "current"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_replicated_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("replicated", serde_json::json!({"target": "offsite"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- annotation ---

    #[test]
    fn test_annotation_match() {
        let file = test_file();
        let mut ctx = MockContext::new();
        ctx.annotations.insert("abc:ai-summarizer".to_string());
        let steps = vec![make_step("annotation", serde_json::json!({"plugin_name": "ai-summarizer"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_annotation_no_match() {
        let file = test_file();
        let ctx = MockContext::new();
        let steps = vec![make_step("annotation", serde_json::json!({"plugin_name": "ai-summarizer"}))];
        let result = evaluate_steps(&steps, &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- on_match variants ---

    #[test]
    fn test_on_match_exclude() {
        let file = test_file();
        let ctx = MockContext::new();
        let mut step = make_step("glob", serde_json::json!({"pattern": "*.pdf"}));
        step.on_match = Some(StepResult::Exclude);
        let result = evaluate_steps(&[step], &file, "file::abc", &StepResult::Include, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    #[test]
    fn test_on_match_continue() {
        let file = test_file();
        let ctx = MockContext::new();
        let mut step1 = make_step("glob", serde_json::json!({"pattern": "*.pdf"}));
        step1.on_match = Some(StepResult::Continue);
        let step2 = make_step("node", serde_json::json!({"node_id": "node-1"}));
        let result = evaluate_steps(&[step1, step2], &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    // --- empty steps ---

    #[test]
    fn test_empty_steps() {
        let file = test_file();
        let ctx = MockContext::new();
        let result = evaluate_steps(&[], &file, "file::abc", &StepResult::Include, &ctx);
        assert_eq!(result, StepResult::Include);
    }

    #[test]
    fn test_empty_steps_exclude_default() {
        let file = test_file();
        let ctx = MockContext::new();
        let result = evaluate_steps(&[], &file, "file::abc", &StepResult::Exclude, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- ancestor inheritance: caller concatenates ancestor + child steps ---

    #[test]
    fn test_ancestor_exclude_overrides_child_include() {
        let file = test_file();
        let ctx = MockContext::new();
        // Ancestor step: exclude all PDFs
        let mut ancestor = make_step("glob", serde_json::json!({"pattern": "*.pdf"}));
        ancestor.on_match = Some(StepResult::Exclude);
        // Child step: include from node-1
        let child = make_step("node", serde_json::json!({"node_id": "node-1"}));
        // Ancestor steps come first → short-circuits with Exclude
        let result = evaluate_steps(&[ancestor, child], &file, "file::abc", &StepResult::Include, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }

    // --- invert with on_match ---

    #[test]
    fn test_invert_with_exclude() {
        let file = test_file();
        let ctx = MockContext::new();
        // Exclude non-PDF files (invert glob *.txt → matches because report.pdf doesn't match *.txt, inverted = true)
        let mut step = make_step("glob", serde_json::json!({"pattern": "*.txt"}));
        step.invert = true;
        step.on_match = Some(StepResult::Exclude);
        let result = evaluate_steps(&[step], &file, "file::abc", &StepResult::Include, &ctx);
        assert_eq!(result, StepResult::Exclude);
    }
}
