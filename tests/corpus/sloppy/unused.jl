module UnusedJulia

function takes_unused(used, unused_arg)
    return used + 1
end

function has_unused_binding(x)
    unused_binding = x + 1
    return x
end

end

