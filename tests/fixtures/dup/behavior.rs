pub fn score_a(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        if *value > 0 {
            total += value * 2;
        } else {
            total -= value;
        }
    }
    total
}

pub fn score_b(items: &[i32]) -> i32 {
    let mut total = 0;
    for value in items {
        if *value > 0 {
            total += value * 2;
        } else {
            total -= value;
        }
    }
    total
}
