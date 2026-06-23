function score_a(values)
    total = 0
    for value in values
        if value > 0
            total += value * 2
        else
            total -= value
        end
    end
    total
end

function score_b(items)
    total = 0
    for value in items
        if value > 0
            total += value * 2
        else
            total -= value
        end
    end
    total
end
