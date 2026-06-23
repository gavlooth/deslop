fn score_alpha(values: &[i32]) -> i32 {
    let positives = values.iter().filter(|value| **value > 0);
    let doubled = positives.map(|value| value * 2);
    let adjusted = doubled.map(|value| value + 3);
    let total: i32 = adjusted.sum();
    if total > 10 { total + 1 } else { total - 1 }
}

fn score_alpha_copy(values: &[i32]) -> i32 {
    let positives = values.iter().filter(|value| **value > 0);
    let doubled = positives.map(|value| value * 2);
    let adjusted = doubled.map(|value| value + 3);
    let total: i32 = adjusted.sum();
    if total > 10 { total + 1 } else { total - 1 }
}

fn score_beta(items: &[i32]) -> i32 {
    let positives = items.iter().filter(|item| **item > 0);
    let doubled = positives.map(|item| item * 2);
    let adjusted = doubled.map(|item| item + 3);
    let total: i32 = adjusted.sum();
    if total > 10 { total + 1 } else { total - 1 }
}

