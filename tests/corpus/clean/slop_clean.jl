module SlopCleanJulia

const DOMAIN_LIMIT = 37

function uses_named_limit(input)
    input + DOMAIN_LIMIT
end

function complete_small_function(input)
    input + 1
end

function documented_reason(input)
    # Domain rule: retain the raw value for reconciliation.
    input
end

end

