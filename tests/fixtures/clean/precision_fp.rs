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

enum LeftArg {
    One,
    Two,
    Three,
}

enum Left {
    One,
    Two,
    Three,
}

enum RightArg {
    Alpha,
    Beta,
    Gamma,
}

enum Right {
    Alpha,
    Beta,
    Gamma,
}

impl From<LeftArg> for Left {
    fn from(value: LeftArg) -> Self {
        match value {
            LeftArg::One => Left::One,
            LeftArg::Two => Left::Two,
            LeftArg::Three => Left::Three,
        }
    }
}

impl From<RightArg> for Right {
    fn from(value: RightArg) -> Self {
        match value {
            RightArg::Alpha => Right::Alpha,
            RightArg::Beta => Right::Beta,
            RightArg::Gamma => Right::Gamma,
        }
    }
}
