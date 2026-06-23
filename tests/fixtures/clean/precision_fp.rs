struct RuleSpec {
    rule: &'static str,
    pattern: &'static str,
    message: &'static str,
}

struct Response {
    status: &'static str,
    value: i32,
}

fn rule_specs() -> Vec<RuleSpec> {
    vec![
        RuleSpec {
            rule: "narrating-comment",
            pattern: r"(import|initialize|return|calculate|compute|convert)",
            message: "comment describes the next statement",
        },
        RuleSpec {
            rule: "magic-number",
            pattern: r"(timeout|limit|threshold|capacity|window)",
            message: "literal should probably be named",
        },
        RuleSpec {
            rule: "incompleteness",
            pattern: r"(todo|unimplemented|placeholder|not implemented)",
            message: "stub text appears in executable code",
        },
    ]
}

fn regex_policy() -> &'static str {
    r"(import|initialize|return|calculate|compute|convert|normalize|serialize|deserialize|validate|compile|execute)"
}

fn ok_left(value: i32) -> Result<Response, String> {
    Ok(Response {
        status: "left",
        value,
    })
}

fn ok_right(value: i32) -> Result<Response, String> {
    Ok(Response {
        status: "right",
        value,
    })
}
