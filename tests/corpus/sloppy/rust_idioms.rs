fn tail_return() -> i32 {
    return 1;
}

fn format_value(value: i32) -> String {
    format!("{}", value)
}

fn closure_forward(values: Vec<i32>) -> Vec<i32> {
    values.into_iter().map(|x| normalize(x)).collect()
}

fn normalize(value: i32) -> i32 {
    value
}

fn cloned_name(name: String) -> String {
    name.clone()
}

fn bind_then_return() -> i32 {
    let answer = 42;
    answer
}

