#[derive(Debug, Clone, PartialEq)]
struct User {
    id: u64,
    name: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Account {
    id: u64,
    name: String,
}

fn explicit_tail_return() -> i32 {
    return 1;
}

fn ownership_required_clone(input: &String) -> (String, usize) {
    let owned = input.clone();
    (owned, input.len())
}

fn early_return(value: i32) -> i32 {
    if value < 0 {
        return 0;
    }
    value + 1
}
