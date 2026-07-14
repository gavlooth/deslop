use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CapabilityAuthority, CapabilitySupport, DialectDeclaration};

pub const LANGUAGE_CONTROL_FLOW_RULE_SCHEMA: &str = "deslop.language-control-flow-rules/1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowSyntaxSelector {
    raw_kind: String,
    text: Option<String>,
}

impl ControlFlowSyntaxSelector {
    pub fn new(raw_kind: impl Into<String>, text: Option<String>) -> Self {
        Self {
            raw_kind: raw_kind.into(),
            text,
        }
    }

    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn matches(&self, raw_kind: &str, text: &str) -> bool {
        self.raw_kind == raw_kind && self.text.as_deref().is_none_or(|value| value == text)
    }

    fn validate(&self) -> Result<(), String> {
        validate_text("control-flow selector raw kind", &self.raw_kind)?;
        if let Some(text) = &self.text {
            validate_text("control-flow selector text", text)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlFlowOwnerRuleKind {
    Callable,
    Initializer,
    ModuleInitializer,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowOwnerRule {
    selector: ControlFlowSyntaxSelector,
    kind: ControlFlowOwnerRuleKind,
    body_field: String,
}

impl ControlFlowOwnerRule {
    pub fn new(
        selector: ControlFlowSyntaxSelector,
        kind: ControlFlowOwnerRuleKind,
        body_field: impl Into<String>,
    ) -> Self {
        Self {
            selector,
            kind,
            body_field: body_field.into(),
        }
    }

    pub fn selector(&self) -> &ControlFlowSyntaxSelector {
        &self.selector
    }

    pub fn kind(&self) -> &ControlFlowOwnerRuleKind {
        &self.kind
    }

    pub fn body_field(&self) -> &str {
        &self.body_field
    }

    fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        validate_text("control-flow owner body field", &self.body_field)?;
        validate_owner_kind(&self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlEvaluationOrder {
    LeftToRight,
    RightToLeft,
    Unspecified,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlLoopForm {
    PreTest,
    PostTest,
    Iterator,
    Infinite,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlAbruptForm {
    Return,
    Break,
    Continue,
    Goto,
    Terminate,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlExceptionalForm {
    Try,
    Throw,
    Rethrow,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlSuspensionForm {
    Await,
    Yield,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ControlFlowAction {
    Sequence,
    Branch {
        condition_field: String,
        consequence_field: String,
        alternative_field: Option<String>,
    },
    Match {
        subject_field: String,
        arm_kind: String,
        arm_body_field: Option<String>,
        guard_field: Option<String>,
    },
    Loop {
        form: ControlLoopForm,
        condition_field: Option<String>,
        body_field: String,
        alternative_field: Option<String>,
        label_kind: Option<String>,
    },
    Abrupt {
        form: ControlAbruptForm,
        value_field: Option<String>,
        label_kind: Option<String>,
    },
    Exceptional {
        form: ControlExceptionalForm,
        body_field: Option<String>,
        handler_kind: Option<String>,
        handler_body_field: Option<String>,
        finally_kind: Option<String>,
        finally_body_field: Option<String>,
    },
    Suspension {
        form: ControlSuspensionForm,
        operand_field: Option<String>,
    },
    OpaqueBoundary {
        reason: String,
    },
    AdapterDefined {
        schema: String,
        name: String,
    },
}

impl ControlFlowAction {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Sequence => Ok(()),
            Self::Branch {
                condition_field,
                consequence_field,
                alternative_field,
            } => {
                validate_text("branch condition field", condition_field)?;
                validate_text("branch consequence field", consequence_field)?;
                validate_optional_text("branch alternative field", alternative_field)
            }
            Self::Match {
                subject_field,
                arm_kind,
                arm_body_field,
                guard_field,
            } => {
                validate_text("match subject field", subject_field)?;
                validate_text("match arm kind", arm_kind)?;
                validate_optional_text("match arm body field", arm_body_field)?;
                validate_optional_text("match guard field", guard_field)
            }
            Self::Loop {
                form: _,
                condition_field,
                body_field,
                alternative_field,
                label_kind,
            } => {
                validate_optional_text("loop condition field", condition_field)?;
                validate_text("loop body field", body_field)?;
                validate_optional_text("loop alternative field", alternative_field)?;
                validate_optional_text("loop label kind", label_kind)
            }
            Self::Abrupt {
                form,
                value_field,
                label_kind,
            } => {
                validate_abrupt_form(form)?;
                validate_optional_text("abrupt value field", value_field)?;
                validate_optional_text("abrupt label kind", label_kind)
            }
            Self::Exceptional {
                form,
                body_field,
                handler_kind,
                handler_body_field,
                finally_kind,
                finally_body_field,
            } => {
                validate_exceptional_form(form)?;
                for (label, field) in [
                    ("exception body field", body_field),
                    ("exception handler kind", handler_kind),
                    ("exception handler body field", handler_body_field),
                    ("exception finally kind", finally_kind),
                    ("exception finally body field", finally_body_field),
                ] {
                    validate_optional_text(label, field)?;
                }
                if *form == ControlExceptionalForm::Try && body_field.is_none() {
                    return Err("try lowering requires a body field".into());
                }
                Ok(())
            }
            Self::Suspension {
                form,
                operand_field,
            } => {
                validate_suspension_form(form)?;
                validate_optional_text("suspension operand field", operand_field)
            }
            Self::OpaqueBoundary { reason } => validate_text("opaque control-flow reason", reason),
            Self::AdapterDefined { schema, name } => {
                validate_adapter_pair("control-flow action", schema, name)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowRule {
    selector: ControlFlowSyntaxSelector,
    action: ControlFlowAction,
}

impl ControlFlowRule {
    pub fn new(selector: ControlFlowSyntaxSelector, action: ControlFlowAction) -> Self {
        Self { selector, action }
    }

    pub fn selector(&self) -> &ControlFlowSyntaxSelector {
        &self.selector
    }

    pub fn action(&self) -> &ControlFlowAction {
        &self.action
    }

    fn validate(&self) -> Result<(), String> {
        self.selector.validate()?;
        self.action.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageControlFlowRulePack {
    schema: String,
    adapter_schema: String,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    dialects: Vec<DialectDeclaration>,
    evaluation_order: Option<ControlEvaluationOrder>,
    owners: Vec<ControlFlowOwnerRule>,
    rules: Vec<ControlFlowRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageControlFlowRulePackWire {
    schema: String,
    adapter_schema: String,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    dialects: Vec<DialectDeclaration>,
    evaluation_order: Option<ControlEvaluationOrder>,
    owners: Vec<ControlFlowOwnerRule>,
    rules: Vec<ControlFlowRule>,
}

impl<'de> Deserialize<'de> for LanguageControlFlowRulePack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageControlFlowRulePackWire::deserialize(deserializer)?;
        let pack = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            support: wire.support,
            authority: wire.authority,
            dialects: wire.dialects,
            evaluation_order: wire.evaluation_order,
            owners: wire.owners,
            rules: wire.rules,
        };
        pack.validate().map_err(D::Error::custom)?;
        Ok(pack)
    }
}

impl LanguageControlFlowRulePack {
    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self::unavailable(adapter_schema, CapabilitySupport::Unknown)
    }

    pub fn unsupported(adapter_schema: impl Into<String>) -> Self {
        Self::unavailable(adapter_schema, CapabilitySupport::Unsupported)
    }

    fn unavailable(adapter_schema: impl Into<String>, support: CapabilitySupport) -> Self {
        Self {
            schema: LANGUAGE_CONTROL_FLOW_RULE_SCHEMA.into(),
            adapter_schema: adapter_schema.into(),
            support,
            authority: None,
            dialects: Vec::new(),
            evaluation_order: None,
            owners: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn provided(
        adapter_schema: impl Into<String>,
        authority: CapabilityAuthority,
        mut dialects: Vec<DialectDeclaration>,
        evaluation_order: ControlEvaluationOrder,
        mut owners: Vec<ControlFlowOwnerRule>,
        mut rules: Vec<ControlFlowRule>,
    ) -> Result<Self, String> {
        dialects.sort();
        owners.sort();
        rules.sort();
        let pack = Self {
            schema: LANGUAGE_CONTROL_FLOW_RULE_SCHEMA.into(),
            adapter_schema: adapter_schema.into(),
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            dialects,
            evaluation_order: Some(evaluation_order),
            owners,
            rules,
        };
        pack.validate()?;
        Ok(pack)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn dialects(&self) -> &[DialectDeclaration] {
        &self.dialects
    }

    pub fn evaluation_order(&self) -> Option<&ControlEvaluationOrder> {
        self.evaluation_order.as_ref()
    }

    pub fn owners(&self) -> &[ControlFlowOwnerRule] {
        &self.owners
    }

    pub fn rules(&self) -> &[ControlFlowRule] {
        &self.rules
    }

    pub fn applies_to(&self, dialect: &str, grammar_id: &str, grammar_version: &str) -> bool {
        self.support == CapabilitySupport::Provided
            && self
                .dialects
                .iter()
                .any(|item| item.matches(dialect, grammar_id, grammar_version))
    }

    pub fn owner_rule(&self, raw_kind: &str, text: &str) -> Option<&ControlFlowOwnerRule> {
        self.owners
            .iter()
            .find(|rule| rule.selector.matches(raw_kind, text))
    }

    pub fn rule(&self, raw_kind: &str, text: &str) -> Option<&ControlFlowRule> {
        self.rules
            .iter()
            .find(|rule| rule.selector.matches(raw_kind, text))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_CONTROL_FLOW_RULE_SCHEMA {
            return Err(format!(
                "unsupported control-flow rule schema {}",
                self.schema
            ));
        }
        validate_text("control-flow adapter schema", &self.adapter_schema)?;
        match (self.support, self.authority) {
            (CapabilitySupport::Provided, Some(authority)) if is_static_authority(authority) => {}
            (CapabilitySupport::Provided, Some(_)) => {
                return Err("control-flow rules require static authority".into());
            }
            (CapabilitySupport::Provided, None) => {
                return Err("provided control-flow rules require authority".into());
            }
            (CapabilitySupport::Unknown | CapabilitySupport::Unsupported, None) => {
                if !self.dialects.is_empty()
                    || self.evaluation_order.is_some()
                    || !self.owners.is_empty()
                    || !self.rules.is_empty()
                {
                    return Err("unavailable control-flow rules retain executable payload".into());
                }
                return Ok(());
            }
            (CapabilitySupport::Unknown | CapabilitySupport::Unsupported, Some(_)) => {
                return Err("unavailable control-flow rules claim authority".into());
            }
        }
        if self.dialects.is_empty() || self.owners.is_empty() || self.rules.is_empty() {
            return Err("provided control-flow rules require dialects, owners, and rules".into());
        }
        validate_canonical_distinct("control-flow dialects", &self.dialects)?;
        for dialect in &self.dialects {
            validate_text("control-flow dialect", dialect.dialect())?;
            validate_text("control-flow grammar id", dialect.grammar_id())?;
            validate_text("control-flow grammar version", dialect.grammar_version())?;
        }
        let evaluation = self
            .evaluation_order
            .as_ref()
            .ok_or_else(|| "provided control-flow rules require evaluation order".to_string())?;
        validate_evaluation_order(evaluation)?;
        validate_canonical_distinct("control-flow owner rules", &self.owners)?;
        validate_canonical_distinct("control-flow rules", &self.rules)?;
        let mut owner_selectors = BTreeSet::new();
        for owner in &self.owners {
            owner.validate()?;
            if !owner_selectors.insert(owner.selector.clone()) {
                return Err("control-flow owner selectors must be distinct".into());
            }
        }
        let mut selectors = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !selectors.insert(rule.selector.clone()) {
                return Err("control-flow rule selectors must be distinct".into());
            }
        }
        Ok(())
    }
}

fn is_static_authority(authority: CapabilityAuthority) -> bool {
    matches!(
        authority,
        CapabilityAuthority::Adapter
            | CapabilityAuthority::LanguageServer
            | CapabilityAuthority::Compiler
    )
}

fn validate_owner_kind(kind: &ControlFlowOwnerRuleKind) -> Result<(), String> {
    if let ControlFlowOwnerRuleKind::AdapterDefined { schema, name } = kind {
        validate_adapter_pair("control-flow owner kind", schema, name)?;
    }
    Ok(())
}

fn validate_evaluation_order(order: &ControlEvaluationOrder) -> Result<(), String> {
    if let ControlEvaluationOrder::AdapterDefined { schema, name } = order {
        validate_adapter_pair("control-flow evaluation order", schema, name)?;
    }
    Ok(())
}

fn validate_abrupt_form(form: &ControlAbruptForm) -> Result<(), String> {
    if let ControlAbruptForm::AdapterDefined { schema, name } = form {
        validate_adapter_pair("abrupt control form", schema, name)?;
    }
    Ok(())
}

fn validate_exceptional_form(form: &ControlExceptionalForm) -> Result<(), String> {
    if let ControlExceptionalForm::AdapterDefined { schema, name } = form {
        validate_adapter_pair("exceptional control form", schema, name)?;
    }
    Ok(())
}

fn validate_suspension_form(form: &ControlSuspensionForm) -> Result<(), String> {
    if let ControlSuspensionForm::AdapterDefined { schema, name } = form {
        validate_adapter_pair("suspension control form", schema, name)?;
    }
    Ok(())
}

fn validate_adapter_pair(label: &str, schema: &str, name: &str) -> Result<(), String> {
    validate_text(&format!("{label} schema"), schema)?;
    validate_text(&format!("{label} name"), name)
}

fn validate_optional_text(label: &str, value: &Option<String>) -> Result<(), String> {
    if let Some(value) = value {
        validate_text(label, value)?;
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(format!("{label} must be nonempty control-free text"));
    }
    Ok(())
}

fn validate_canonical_distinct<T: Ord>(label: &str, values: &[T]) -> Result<(), String> {
    for pair in values.windows(2) {
        match pair[0].cmp(&pair[1]) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => return Err(format!("{label} contain duplicates")),
            std::cmp::Ordering::Greater => {
                return Err(format!("{label} are not in canonical order"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    type JsonMutation = Box<dyn Fn(&mut Value)>;

    fn complete_pack() -> LanguageControlFlowRulePack {
        LanguageControlFlowRulePack::provided(
            "deslop-lang-adapter/test-3",
            CapabilityAuthority::Adapter,
            vec![DialectDeclaration::new("test", "tree-sitter-test", "1.0.0")],
            ControlEvaluationOrder::LeftToRight,
            vec![ControlFlowOwnerRule::new(
                ControlFlowSyntaxSelector::new("function", None),
                ControlFlowOwnerRuleKind::Callable,
                "body",
            )],
            vec![
                ControlFlowRule::new(
                    ControlFlowSyntaxSelector::new("block", None),
                    ControlFlowAction::Sequence,
                ),
                ControlFlowRule::new(
                    ControlFlowSyntaxSelector::new("if", None),
                    ControlFlowAction::Branch {
                        condition_field: "condition".into(),
                        consequence_field: "consequence".into(),
                        alternative_field: Some("alternative".into()),
                    },
                ),
                ControlFlowRule::new(
                    ControlFlowSyntaxSelector::new("return", None),
                    ControlFlowAction::Abrupt {
                        form: ControlAbruptForm::Return,
                        value_field: Some("value".into()),
                        label_kind: None,
                    },
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn control_flow_rule_pack_is_strict_canonical_and_applicable() {
        let pack = complete_pack();
        assert_eq!(pack.schema(), LANGUAGE_CONTROL_FLOW_RULE_SCHEMA);
        assert!(pack.applies_to("test", "tree-sitter-test", "1.0.0"));
        assert!(!pack.applies_to("test", "tree-sitter-test", "2.0.0"));
        assert!(pack.owner_rule("function", "fn f() {}").is_some());
        assert!(matches!(
            pack.rule("if", "if x {}").unwrap().action(),
            ControlFlowAction::Branch { .. }
        ));

        let bytes = serde_json::to_vec(&pack).unwrap();
        let decoded: LanguageControlFlowRulePack = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, pack);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);

        let mut unknown: Value = serde_json::from_slice(&bytes).unwrap();
        unknown["extra"] = Value::Bool(true);
        assert!(serde_json::from_value::<LanguageControlFlowRulePack>(unknown).is_err());
    }

    #[test]
    fn unavailable_and_corrupted_rule_packs_fail_closed() {
        for pack in [
            LanguageControlFlowRulePack::unknown("adapter/3"),
            LanguageControlFlowRulePack::unsupported("adapter/3"),
        ] {
            pack.validate().unwrap();
            assert!(pack.dialects().is_empty());
            assert!(pack.owners().is_empty());
            assert!(pack.rules().is_empty());
        }

        let original = serde_json::to_value(complete_pack()).unwrap();
        let mutations: Vec<JsonMutation> = vec![
            Box::new(|value| value["schema"] = Value::String("rules/0".into())),
            Box::new(|value| value["authority"] = Value::String("runtime-verification".into())),
            Box::new(|value| value["dialects"] = serde_json::json!([])),
            Box::new(|value| value["owners"] = serde_json::json!([])),
            Box::new(|value| value["rules"] = serde_json::json!([])),
            Box::new(|value| value["rules"].as_array_mut().unwrap().swap(0, 1)),
            Box::new(|value| {
                value["support"] = Value::String("unknown".into());
                value["authority"] = Value::Null;
            }),
        ];
        for mutate in mutations {
            let mut value = original.clone();
            mutate(&mut value);
            assert!(serde_json::from_value::<LanguageControlFlowRulePack>(value).is_err());
        }
    }
}
