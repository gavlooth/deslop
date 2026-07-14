# @generated
module AdapterMatrix

export compute

@generated

function compute(π::Int, values)
    # line comment
    #= block comment =#
    total = π * 2
    message = "total = $total"

    for value in values
        if value > 0
            total += helper(value)
        end
    end

    quoted = quote
        hidden(total)
    end
    @time helper(total)
    return total
end

end
