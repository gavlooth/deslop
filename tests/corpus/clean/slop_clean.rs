const DOMAIN_LIMIT: i32 = 37;

fn uses_named_limit(input: i32) -> i32 {
    input + DOMAIN_LIMIT
}

fn complete_small_function(input: i32) -> i32 {
    input + 1
}

fn documented_reason(input: i32) -> i32 {
    // SAFETY: keep this branch explicit for auditability.
    input + 1
}

