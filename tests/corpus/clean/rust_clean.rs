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

enum SourceMode {
    Fast,
    Slow,
    Off,
}

enum RuntimeMode {
    Fast,
    Slow,
    Off,
}

enum SourceLevel {
    Low,
    Medium,
    High,
}

enum RuntimeLevel {
    Low,
    Medium,
    High,
}

impl From<SourceMode> for RuntimeMode {
    fn from(value: SourceMode) -> Self {
        match value {
            SourceMode::Fast => RuntimeMode::Fast,
            SourceMode::Slow => RuntimeMode::Slow,
            SourceMode::Off => RuntimeMode::Off,
        }
    }
}

impl From<SourceLevel> for RuntimeLevel {
    fn from(value: SourceLevel) -> Self {
        match value {
            SourceLevel::Low => RuntimeLevel::Low,
            SourceLevel::Medium => RuntimeLevel::Medium,
            SourceLevel::High => RuntimeLevel::High,
        }
    }
}
